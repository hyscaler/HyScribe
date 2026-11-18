// SPDX-License-Identifier: AGPL-3.0-or-later

//! The attachment of an upload: `media_handle_upload()`, which hs-admin's
//! upload screens call, and the attachments controller's
//! `insert_attachment()`, each up to its insert ([`Prepared`]), then the
//! insert and the metadata in two actions of the writer, the sizes made
//! between them on the worker pool, as PHP writes the rows before it
//! makes the sizes.

use hyscribe_db::{DbError, Meta, Write, Writer};
use hyscribe_image::header::getimagesize;
use hyscribe_image::meta::wp_read_image_metadata;
use hyscribe_image::subsizes::Generated;
use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, is_empty_string, lookup, php_trim, stripslashes, wp_basename};
use hyscribe_render::date::format_unix;
use hyscribe_render::formatting::{sanitize_text_field, sanitize_title};
use hyscribe_rest::Upload;
use hyscribe_write::options::update_metadata;
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

/// `wp_read_image_metadata( $file )` of an image upload, as the attachment
/// reads it before it is inserted: `None` for what is not an image, 501
/// for EXIF and XMP PHP would read in ways not ported.
fn image_meta(site: &Site, handled: &Handled) -> Result<ImageMeta, Refused> {
    let size = getimagesize(&handled.bytes).filter(|_| handled.mime.starts_with("image/"));
    let host = hyscribe_render::link::host(site.raw_option("siteurl"));
    size.map(|size| wp_read_image_metadata(&handled.bytes, &size, &host))
        .transpose()
        .map_err(|_| Refused::Unported)
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
    ) -> Result<(Handled, ImageMeta), Refused> {
        let handled = self.handle(site, sent, (time, who), sideload)?;
        match image_meta(site, &handled) {
            Ok(meta) => Ok((handled, meta)),
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
        let (handled, meta) = self.written(site, sent, (time, who), false)?;
        // The name as sent, its type's extension given, without it.
        let base = wp_basename(&handled.name);
        let dot = base.iter().rposition(|&b| b == b'.').filter(|&at| at > 0);
        let stem = dot.and_then(|at| base.get(..at)).unwrap_or(&base);
        let [title, caption, alt] = image_fields(meta.as_deref());
        let title = title.unwrap_or_else(|| sanitize_text_field(stem));
        // wp_insert_post() unslashes what media_handle_upload() gives it.
        let args = PostArgs {
            title: stripslashes(&title),
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
        let (handled, meta) = self.written(site, sent, (time, who), !form)?;
        let given = |value: &Option<Vec<u8>>| value.clone().filter(|value| !is_empty_string(value));
        let [meta_title, meta_caption, alt] = image_fields(meta.as_deref());
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
    /// made and its metadata saved in another: its ID. The upload is
    /// deleted again where the insert stops.
    pub(crate) fn completed(
        &self,
        site: &Site,
        writer: &Writer,
        ctx: &Context,
        prepared: &Prepared,
    ) -> Result<u64, Refused> {
        let handle = Handle::current();
        let inserted =
            handle.block_on(async { inserted(writer.begin().await, ctx, prepared).await });
        let id = inserted.map_err(|err| {
            self.discard(&prepared.handled);
            Refused::from(err)
        })?;
        self.added(id);
        let generated = self
            .generate(site, &prepared.handled, id)
            .map_err(|err| Refused::Failed(err.to_string()))?;
        let saved = handle.block_on(async { saved(writer.begin().await, id, &generated).await });
        saved.map_err(Refused::from)?;
        Ok(id)
    }
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
/// begun.
pub(crate) async fn saved(
    write: Result<Write<'_>, DbError>,
    id: u64,
    generated: &Generated,
) -> Result<(), hyscribe_write::Error> {
    let mut write = write?;
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
