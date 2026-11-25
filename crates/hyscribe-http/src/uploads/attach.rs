// SPDX-License-Identifier: AGPL-3.0-or-later

//! The attachment of an upload: `media_handle_upload()`, which hs-admin's
//! upload screens call, and the attachments controller's
//! `insert_attachment()`, each up to its insert ([`Prepared`]), then the
//! insert and the metadata in two actions of the writer, the sizes made
//! between them on the worker pool, as PHP writes the rows before it
//! makes the sizes.

use hyscribe_db::{DbError, Meta, Write, Writer};
use hyscribe_image::getid3::{wp_read_audio_metadata, wp_read_video_metadata};
use hyscribe_image::header::getimagesize;
use hyscribe_image::meta::wp_read_image_metadata;
use hyscribe_image::subsizes::{Cover, Generated};
use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, is_empty_string, lookup, php_trim, stripslashes, wp_basename};
use hyscribe_render::date::format_unix;
use hyscribe_render::formatting::{sanitize_text_field, sanitize_title};
use hyscribe_render::number_format;
use hyscribe_rest::Upload;
use hyscribe_write::options::{add_metadata, update_metadata};
use hyscribe_write::posts::{Context, PostArgs, wp_insert_attachment};
use tokio::runtime::Handle;

use super::{Handled, Refused, Sent, Uploads, Who};
use crate::Site;

/// An upload written to its folder, and the attachment to insert for it.
#[derive(Debug)]
pub(crate) struct Prepared {
    /// The file written.
    pub(crate) handled: Handled,
    /// `wp_insert_attachment()`'s arguments.
    pub(crate) args: PostArgs,
    /// The image's own alt text, saved where it is not blank.
    pub(crate) alt: Vec<u8>,
    /// The REST request's `alt_text`, saved whatever it is.
    pub(crate) alt_text: Option<Vec<u8>>,
}

/// `$time` of an upload attached to `parent`: its date, unless a page's
/// or undated, else `current_time( 'mysql' )` at `now`.
pub(crate) fn upload_time(site: &Site, parent: Option<Post>, now: i64) -> String {
    let dated = |post: &Post| {
        let year = post.date.get(..4).unwrap_or_default();
        post.post_type != "page" && Value::String(year.into()).to_int() > 0
    };
    match parent.filter(dated) {
        Some(post) => post.date,
        None => format_unix("Y-m-d H:i:s", now, site.zone()),
    }
}

/// An image's `image_meta`, `None` for what is not an image.
type ImageMeta = Option<Vec<(Key, Value)>>;

/// What an upload is read as before it is inserted: the image's metadata,
/// and an audio file's as `wp_read_audio_metadata()` reads it before its
/// filter.
#[derive(Default)]
struct Read {
    image: ImageMeta,
    audio: Option<Vec<(Key, Value)>>,
}

/// `wp_read_image_metadata( $file )` of an image upload, and getID3's
/// reading of audio and video, before the attachment is inserted (a PDF
/// gets no preview, ADR 0020): 501 for EXIF and XMP PHP would read in ways
/// not ported, and for audio and video getID3 reads in ways not ported.
fn read_upload(site: &Site, handled: &Handled) -> Result<Read, Refused> {
    let kind = handled.mime.as_str();
    let host = hyscribe_render::link::host(site.raw_option("siteurl"));
    let mut read = Read::default();
    if kind.starts_with("video/") {
        wp_read_video_metadata(&handled.bytes).map_err(|_| Refused::Unported)?;
    } else if kind.starts_with("audio/") {
        let entries =
            wp_read_audio_metadata(&handled.bytes, &host).map_err(|_| Refused::Unported)?;
        read.audio = Some(entries);
    }
    let size = getimagesize(&handled.bytes).filter(|_| handled.mime.starts_with("image/"));
    read.image = size
        .map(|size| wp_read_image_metadata(&handled.bytes, &size, &host))
        .transpose()
        .map_err(|_| Refused::Unported)?;
    Ok(read)
}

/// The text of an audio attachment: its title and what
/// `media_handle_upload()` writes in its content from the tags of `meta`.
fn audio_text(meta: &[(Key, Value)], name: Vec<u8>) -> Result<(Vec<u8>, Vec<u8>), Refused> {
    let tag = |field| {
        lookup(meta, field)
            .map(Value::to_php_string)
            .filter(|value| !is_empty_string(value))
    };
    let title = tag("title").unwrap_or(name);
    let (album, artist) = (tag("album"), tag("artist"));
    let join = |parts: &[&[u8]]| parts.concat();
    let mut content = match (!is_empty_string(&title), &album, &artist) {
        (true, Some(album), Some(artist)) => {
            join(&[b"\"", &title, b"\" from ", album, b" by ", artist, b"."])
        }
        (true, Some(album), None) => join(&[b"\"", &title, b"\" from ", album, b"."]),
        (true, None, Some(artist)) => join(&[b"\"", &title, b"\" by ", artist, b"."]),
        (true, None, None) => join(&[b"\"", &title, b"\"."]),
        (false, Some(album), Some(artist)) => join(&[album, b" by ", artist, b"."]),
        (false, Some(album), None) => join(&[album, b"."]),
        (false, None, Some(artist)) => join(&[artist, b"."]),
        (false, None, None) => Vec::new(),
    };
    if let Some(year) = tag("year") {
        let year = Value::String(year).to_int();
        content.extend(format!(" Released: {year}.").into_bytes());
    }
    if let Some(track) = tag("track_number") {
        let mut parts = track
            .split(|&b| b == b'/')
            .map(|part| Value::String(part.to_vec()));
        let (first, second) = (parts.next(), parts.next());
        // number_format() of a number past a float's exact integers is
        // not ported.
        let exact = |value: &Value| value.to_float().abs() < 1e15;
        if ![&first, &second].into_iter().flatten().all(exact) {
            return Err(Refused::Unported);
        }
        let format = |value: &Value| number_format(value.to_float().round() as i128);
        match (first, second) {
            (Some(first), Some(second)) if first.is_numeric() && second.is_numeric() => content
                .extend(format!(" Track {} of {}.", format(&first), format(&second)).into_bytes()),
            (Some(first), _) if first.is_numeric() => {
                content.extend(format!(" Track {}.", format(&first)).into_bytes());
            }
            _ => {}
        }
    }
    if let Some(genre) = tag("genre") {
        content.extend([b" Genre: ".as_slice(), &genre, b"."].concat());
    }
    Ok((title, content))
}

/// The fields of an image's metadata an attachment takes: its title, if
/// not blank or a number as a slug; its caption and alt text, if not blank.
fn image_fields(meta: Option<&[(Key, Value)]>) -> [Option<Vec<u8>>; 3] {
    let field = |name| {
        let found = meta
            .and_then(|meta| lookup(meta, name))
            .map(Value::to_php_string);
        found.filter(|value| !php_trim(value).is_empty())
    };
    let title = field("title").filter(|title| !Value::String(sanitize_title(title)).is_numeric());
    [title, field("caption"), field("alt")]
}

impl Uploads {
    /// The upload written and its metadata read, deleted again where the
    /// metadata is not ported.
    fn written(
        &self,
        site: &Site,
        sent: Sent,
        (time, who): (&str, Who),
        sideload: bool,
    ) -> Result<(Handled, Read), Refused> {
        let handled = self.handle(site, sent, (time, who), sideload)?;
        match read_upload(site, &handled) {
            Ok(read) => Ok((handled, read)),
            Err(refused) => {
                self.discard(&handled);
                Err(refused)
            }
        }
    }

    /// `media_handle_upload( 'async-upload', $post_id )` of `sent` up to its
    /// insert, `time` and `who` its `_wp_handle_upload()`'s.
    pub(crate) fn media_handle_upload(
        &self,
        site: &Site,
        sent: Sent,
        post_id: u64,
        (time, who): (&str, Who),
    ) -> Result<Prepared, Refused> {
        let (handled, read) = self.written(site, sent, (time, who), false)?;
        // The name as sent, its type's extension given, without it.
        let base = wp_basename(&handled.name);
        let dot = base.iter().rposition(|&b| b == b'.').filter(|&at| at > 0);
        let stem = dot.and_then(|at| base.get(..at)).unwrap_or(&base);
        let [title, caption, alt] = image_fields(read.image.as_deref());
        let title = title.unwrap_or_else(|| sanitize_text_field(stem));
        // An audio file's title and text come from its tags, as the
        // filter of wp_read_audio_metadata() leaves them.
        let (title, content) = match read.audio {
            Some(meta) => {
                let format = lookup(&meta, "fileformat").map(Value::to_php_string);
                let format = format.map(|format| String::from_utf8_lossy(&format).into_owned());
                let meta = self
                    .hooks
                    .wp_read_audio_metadata(Value::Array(meta), format.as_deref());
                let text = audio_text(meta.as_array().unwrap_or_default(), title);
                text.inspect_err(|_| self.discard(&handled))?
            }
            None => (title, Vec::new()),
        };
        // wp_insert_post() unslashes what media_handle_upload() gives it.
        let args = PostArgs {
            title: stripslashes(&title),
            content: stripslashes(&content),
            excerpt: stripslashes(&caption.unwrap_or_default()),
            parent: post_id,
            ..PostArgs::default()
        };
        Ok(Prepared {
            handled,
            args,
            alt: alt.unwrap_or_default(),
            alt_text: None,
        })
    }

    /// The attachments controller's `insert_attachment()` of `sent`, a
    /// form's file or else the body, with `upload`'s fields, up to its
    /// insert.
    pub(crate) fn rest_upload(
        &self,
        site: &Site,
        (sent, form): (Sent, bool),
        upload: &Upload,
        (time, who): (&str, Who),
    ) -> Result<Prepared, Refused> {
        let name = sent.name.clone();
        let (handled, read) = self.written(site, sent, (time, who), !form)?;
        let given = |value: &Option<Vec<u8>>| value.clone().filter(|value| !is_empty_string(value));
        let [meta_title, meta_caption, alt] = image_fields(read.image.as_deref());
        // A form's name as sent without its extension, else the file's.
        let dot = name.iter().rposition(|&b| b == b'.');
        let stem = dot.and_then(|at| name.get(..at));
        let sent_stem = || {
            stem.filter(|stem| form && !stem.is_empty())
                .map(<[u8]>::to_vec)
        };
        let title = given(&upload.title).or(meta_title).or_else(sent_stem);
        let title = title.unwrap_or_else(|| {
            let base = handled.file.rsplit('/').next().unwrap_or_default();
            let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
            stem.as_bytes().to_vec()
        });
        // A caption PHP takes as empty is kept where the image has none.
        let caption = given(&upload.caption)
            .or(meta_caption)
            .or(upload.caption.clone());
        let args = PostArgs {
            title,
            excerpt: caption.unwrap_or_default(),
            content: upload.description.clone().unwrap_or_default(),
            parent: upload.post,
            ..PostArgs::default()
        };
        Ok(Prepared {
            handled,
            args,
            alt: alt.unwrap_or_default(),
            alt_text: upload.alt_text.clone(),
        })
    }

    /// The attachment of `prepared` inserted in one action, then its sizes
    /// made, a cover picture made an attachment of its own, and its
    /// metadata saved: its ID. The upload is deleted again where the insert
    /// stops.
    pub(crate) fn completed(
        &self,
        site: &Site,
        writer: &Writer,
        ctx: &Context,
        prepared: &Prepared,
    ) -> Result<u64, Refused> {
        let actions = Actions::Pool(writer);
        Handle::current().block_on(self.complete(site, actions, ctx, prepared))
    }

    /// [`Uploads::completed`] with its actions run by `actions`.
    pub(crate) async fn complete(
        &self,
        site: &Site,
        mut actions: Actions<'_>,
        ctx: &Context,
        prepared: &Prepared,
    ) -> Result<u64, Refused> {
        let inserted = inserted(actions.begin().await, ctx, prepared).await;
        let id = inserted.map_err(|err| {
            self.discard(&prepared.handled);
            Refused::from(err)
        })?;
        self.added(id);
        self.finish(site, &mut actions, ctx, &prepared.handled, id)
            .await?;
        Ok(id)
    }

    /// `wp_generate_attachment_metadata( $id, $file )` of the attachment
    /// `id` of `handled`, its cover picture made, and the metadata saved.
    async fn finish(
        &self,
        site: &Site,
        actions: &mut Actions<'_>,
        ctx: &Context,
        handled: &Handled,
        id: u64,
    ) -> Result<(), Refused> {
        let generated = self
            .generate(site, handled, id)
            .map_err(|err| Refused::Failed(err.to_string()))?;
        let thumbnail = match &generated.cover {
            Some(cover) => Box::pin(self.cover(site, actions, ctx, handled, cover)).await?,
            None => None,
        };
        saved(actions.begin().await, id, &generated, thumbnail).await?;
        Ok(())
    }

    /// The cover picture `wp_generate_attachment_metadata()` gives an audio
    /// or video file `handled` as its thumbnail: the attachment made of the
    /// same picture before (its `_cover_hash`), else a new one, written as
    /// `wp_upload_bits()` writes it; none where that fails.
    async fn cover(
        &self,
        site: &Site,
        actions: &mut Actions<'_>,
        ctx: &Context,
        handled: &Handled,
        cover: &Cover,
    ) -> Result<Option<u64>, Refused> {
        let hash = hyscribe_phpser::md5(&cover.data);
        if let Some(id) = found(actions.begin().await, &hash, &cover.mime).await? {
            return Ok(Some(id));
        }
        let ext = cover_extension(&cover.mime);
        let base = String::from_utf8_lossy(&wp_basename(handled.file.as_bytes())).replace('.', "-");
        let name = format!("{base}-image.{ext}");
        let time = upload_time(site, None, ctx.now);
        let bits = cover.data.clone().into();
        let Some(uploaded) = self.upload_bits(site, (&name, &cover.mime), &bits, &time) else {
            return Ok(None);
        };
        let args = PostArgs {
            mime_type: uploaded.mime.clone(),
            ..PostArgs::default()
        };
        let args = self.hooks.attachment_thumbnail_args(args, &uploaded);
        let id = covered(actions.begin().await, ctx, args, &uploaded, &hash).await?;
        self.added(id);
        self.finish(site, actions, ctx, &uploaded, id).await?;
        Ok(Some(id))
    }
}

/// The extension `wp_generate_attachment_metadata()` gives a cover picture
/// of `mime`.
fn cover_extension(mime: &[u8]) -> &'static str {
    match mime {
        b"image/gif" => "gif",
        b"image/png" => "png",
        b"image/webp" => "webp",
        _ => "jpg",
    }
}

/// Where an upload's actions run: on the writer's pool, or in a test's
/// transaction.
pub(crate) enum Actions<'a> {
    /// The writer's own connections.
    Pool(&'a Writer),
    /// A connection already in a transaction, and the action, counted from
    /// 0, that fails after the given number of statements.
    #[cfg(test)]
    On(
        &'a Writer,
        &'a mut hyscribe_db::Connection,
        Option<(usize, usize)>,
    ),
}

impl Actions<'_> {
    /// Starts an action.
    async fn begin(&mut self) -> Result<Write<'_>, DbError> {
        match self {
            Actions::Pool(writer) => writer.begin().await,
            #[cfg(test)]
            Actions::On(writer, conn, fail) => {
                let mut write = writer.begin_on(conn).await;
                if let (Ok(write), Some((0, statements))) = (&mut write, *fail) {
                    write.fail_after(statements);
                }
                *fail = fail.map(|(action, statements)| (action.wrapping_sub(1), statements));
                write
            }
        }
    }
}

/// The attachment of the same cover picture made before, by its `hash`
/// and type, looked for once begun.
async fn found(
    write: Result<Write<'_>, DbError>,
    hash: &str,
    mime: &[u8],
) -> Result<Option<u64>, hyscribe_write::Error> {
    Ok(write?.cover_attachment(hash, mime).await?)
}

/// The cover attachment of `args` for the picture `uploaded` inserted as one
/// action, with its `_cover_hash`, once begun.
async fn covered(
    write: Result<Write<'_>, DbError>,
    ctx: &Context,
    args: PostArgs,
    uploaded: &Handled,
    hash: &str,
) -> Result<u64, hyscribe_write::Error> {
    let mut write = write?;
    let id = wp_insert_attachment(&mut write, ctx, args, &uploaded.file).await?;
    let hash = Value::String(hash.as_bytes().to_vec());
    add_metadata(&mut write, Meta::Post, id, "_cover_hash", &hash, false).await?;
    write.commit().await.map(|()| id).map_err(Into::into)
}

/// The attachment of `prepared` inserted as one action, once begun.
pub(crate) async fn inserted(
    write: Result<Write<'_>, DbError>,
    ctx: &Context,
    prepared: &Prepared,
) -> Result<u64, hyscribe_write::Error> {
    let mut write = write?;
    let id = attach(&mut write, ctx, prepared).await?;
    write.commit().await.map(|()| id).map_err(Into::into)
}

/// What `generate()` made of the attachment `id` saved as one action, once
/// begun, after its cover picture's `thumbnail`.
pub(crate) async fn saved(
    write: Result<Write<'_>, DbError>,
    id: u64,
    generated: &Generated,
    thumbnail: Option<u64>,
) -> Result<(), hyscribe_write::Error> {
    let mut write = write?;
    if let Some(thumbnail) = thumbnail {
        let any = Value::String(Vec::new());
        let thumbnail = Value::Int(thumbnail as i64);
        update_metadata(
            &mut write,
            Meta::Post,
            id,
            "_thumbnail_id",
            &thumbnail,
            &any,
        )
        .await?;
    }
    save(&mut write, id, generated).await?;
    write.commit().await.map_err(Into::into)
}

/// `wp_insert_attachment()` of `prepared` in `write`, then its alt text:
/// the image's where not blank, then the request's as given.
pub(crate) async fn attach(
    write: &mut Write<'_>,
    ctx: &Context,
    prepared: &Prepared,
) -> Result<u64, hyscribe_write::Error> {
    let handled = &prepared.handled;
    let args = PostArgs {
        mime_type: handled.mime.clone(),
        guid: handled.url.clone(),
        ..prepared.args.clone()
    };
    let id = wp_insert_attachment(write, ctx, args, &handled.file).await?;
    let alts = [
        Some(&prepared.alt).filter(|alt| !alt.is_empty()),
        prepared.alt_text.as_ref(),
    ];
    let any = Value::String(Vec::new());
    for alt in alts.into_iter().flatten() {
        let alt = Value::String(sanitize_text_field(alt));
        let key = "_wp_attachment_image_alt";
        update_metadata(write, Meta::Post, id, key, &alt, &any).await?;
    }
    Ok(id)
}

/// `wp_update_attachment_metadata()` of what `generate()` made, and the
/// `-scaled` or `-rotated` copy as the attached file, in `write`.
pub(crate) async fn save(
    write: &mut Write<'_>,
    id: u64,
    generated: &Generated,
) -> Result<(), hyscribe_write::Error> {
    let any = Value::String(Vec::new());
    if let Some(file) = &generated.attached_file {
        let file = Value::String(file.as_bytes().to_vec());
        update_metadata(write, Meta::Post, id, "_wp_attached_file", &file, &any).await?;
    }
    let key = "_wp_attachment_metadata";
    update_metadata(write, Meta::Post, id, key, &generated.meta, &any).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
