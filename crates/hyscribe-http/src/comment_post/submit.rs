// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_handle_comment_submission()` over the action's `Write`, in two parts
//! around the post's status and `read_post`, worked out on the blocking thread.

use hyscribe_db::{DbError, PostFields, Write};
use hyscribe_kses::strip_tags;
use hyscribe_phpser::{Value, absint, is_empty_string, php_trim};
use hyscribe_render::comments_open_at;
use hyscribe_render::formatting::is_email;
use hyscribe_write::comments::{Commenter, Context, NewComment, Refused, new_comment};

/// Why a comment is not saved: `wp_die()` with a status and message, or a
/// plain `exit` for an error without a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Silent,
    Died(u16, &'static str),
}

/// The fields of `wp_unslash( $_POST )` the submission reads.
pub(super) struct Fields {
    /// `comment_post_ID`, as `(int)` casts it.
    pub(super) post_id: i64,
    author: Vec<u8>,
    email: Vec<u8>,
    url: Vec<u8>,
    content: Vec<u8>,
    /// `comment_parent`, if set, as `absint()` casts it.
    parent: Option<u64>,
}

impl Fields {
    pub(super) fn read(form: &Value) -> Fields {
        let text = |name| match form.get_str(name) {
            Some(Value::String(text)) => php_trim(text).to_vec(),
            _ => Vec::new(),
        };
        let author = text("author");
        Fields {
            post_id: form.get_str("comment_post_ID").map_or(0, Value::to_int),
            author: php_trim(&strip_tags(&author)).to_vec(),
            email: text("email"),
            url: text("url"),
            content: text("comment"),
            parent: form.get_str("comment_parent").map(absint),
        }
    }
}

const REPLY: &str = "Sorry, replies to unapproved comments are not allowed.";

/// The post commented on, after the reply check: `None` for none, which
/// includes a post without a `comment_status`.
pub(super) async fn post(
    write: &mut Write<'_>,
    fields: &Fields,
) -> Result<Result<Option<PostFields>, Failure>, DbError> {
    if let Some(parent) = fields.parent.filter(|&parent| parent != 0) {
        let comment = write.comment(parent).await?;
        let held = |approved: String| Value::String(approved.into_bytes()).to_int() == 0;
        if comment.is_none_or(|comment| held(comment.approved)) {
            return Ok(Err(Failure::Died(403, REPLY)));
        }
    }
    let post = match u64::try_from(fields.post_id) {
        Ok(id) => write.post(id).await?,
        Err(_) => None,
    };
    Ok(Ok(post.filter(|post| {
        !is_empty_string(post.comment_status.as_bytes())
    })))
}

/// Who comments and from where.
pub(super) struct Poster<'a> {
    /// The current user, 0 for a visitor.
    pub(super) user: u64,
    pub(super) commenter: Commenter,
    /// `current_user_can( 'read_post', $comment_post_id )`.
    pub(super) read_post: bool,
    /// `get_post_status()` of the post.
    pub(super) status: &'a str,
    /// The `wp-postpass_` cookie, if sent.
    pub(super) postpass: Option<&'a [u8]>,
    /// `$_SERVER['REMOTE_ADDR']`.
    pub(super) ip: &'a [u8],
    /// `$_SERVER['HTTP_USER_AGENT']`.
    pub(super) agent: &'a [u8],
    pub(super) context: Context<'a>,
}

/// `wp_check_comment_data_max_lengths()`, over the core schema's columns.
const MAX_LENGTHS: [(usize, &str); 4] = [
    (245, "<strong>Error:</strong> Your name is too long."),
    (
        100,
        "<strong>Error:</strong> Your email address is too long.",
    ),
    (200, "<strong>Error:</strong> Your URL is too long."),
    (65525, "<strong>Error:</strong> Your comment is too long."),
];

/// The rest of the submission on `post`, which exists: the new comment's
/// ID, or why there is none.
pub(super) async fn submit(
    write: &mut Write<'_>,
    mut fields: Fields,
    post: &PostFields,
    poster: &Poster<'_>,
) -> Result<Result<u64, Failure>, DbError> {
    let status = poster.status;
    if status == "private" && !poster.read_post {
        return Ok(Err(Failure::Silent));
    }
    let names = ["close_comments_for_old_posts", "close_comments_days_old"];
    let [close, days_old] = options(write, names).await?;
    let open = (&*post.comment_status, &*post.post_type, &*post.date_gmt);
    if !comments_open_at((&close, &days_old), open, poster.context.now) {
        return Ok(Err(Failure::Died(
            403,
            "Sorry, comments are closed for this item.",
        )));
    }
    if status == "trash" {
        return Ok(Err(Failure::Silent));
    }
    if status != "publish" && status != "private" {
        return Ok(Err(if poster.read_post {
            Failure::Died(403, "Sorry, comments are not allowed for this item.")
        } else {
            Failure::Silent
        }));
    }
    if password_required(post, poster.postpass) {
        return Ok(Err(Failure::Silent));
    }
    let set = async |write: &mut Write<'_>, name| {
        let value = write.option_value(name).await?;
        Ok::<_, DbError>(!is_empty_string(&value.unwrap_or_default()))
    };
    let user = match poster.user {
        0 => None,
        id => write.user(id).await?,
    };
    let user_id = match user {
        Some((login, user)) => {
            fields.author = if is_empty_string(&user.display_name) {
                login
            } else {
                user.display_name
            };
            fields.email = user.email;
            fields.url = user.url;
            poster.user
        }
        None if set(write, "comment_registration").await? => {
            let message = "Sorry, you must be logged in to comment.";
            return Ok(Err(Failure::Died(403, message)));
        }
        None => 0,
    };
    if user_id == 0 && set(write, "require_name_email").await? {
        if fields.email.is_empty() || fields.author.is_empty() {
            let message = "<strong>Error:</strong> Please fill the required fields.";
            return Ok(Err(Failure::Died(200, message)));
        }
        if !is_email(&fields.email) {
            let message = "<strong>Error:</strong> Please enter a valid email address.";
            return Ok(Err(Failure::Died(200, message)));
        }
    }
    if fields.content.is_empty() {
        let message = "<strong>Error:</strong> Please type your comment text.";
        return Ok(Err(Failure::Died(200, message)));
    }
    let lengths = [&fields.author, &fields.email, &fields.url, &fields.content];
    if let Some((_, message)) =
        (lengths.iter().zip(MAX_LENGTHS)).find(|(f, (max, _))| f.len() > *max)
    {
        return Ok(Err(Failure::Died(200, message.1)));
    }
    let comment = NewComment {
        post_id: u64::try_from(fields.post_id).unwrap_or_default(),
        author: fields.author,
        author_email: fields.email,
        author_url: fields.url,
        author_ip: poster.ip.to_vec(),
        content: fields.content,
        agent: poster.agent.to_vec(),
        parent: fields.parent.unwrap_or_default(),
        user_id,
        ..NewComment::default()
    };
    let saved = new_comment(write, poster.context, comment, poster.commenter).await?;
    Ok(saved.map_err(|refused| match refused {
        Refused::Duplicate => Failure::Died(
            409,
            "Duplicate comment detected; it looks as though you&#8217;ve already said that!",
        ),
        Refused::Flood => Failure::Died(429, "You are posting comments too quickly. Slow down."),
    }))
}

/// `get_option()` of each of `names`, read in the action: empty for a
/// missing option.
pub(super) async fn options<const N: usize>(
    write: &mut Write<'_>,
    names: [&str; N],
) -> Result<[Vec<u8>; N], DbError> {
    let mut values = [const { Vec::new() }; N];
    for (value, name) in values.iter_mut().zip(names) {
        *value = write.option_value(name).await?.unwrap_or_default();
    }
    Ok(values)
}

/// `post_password_required()`: a password, unless the `wp-postpass_`
/// cookie holds its portable phpass hash.
fn password_required(post: &PostFields, cookie: Option<&[u8]>) -> bool {
    if is_empty_string(&post.password) {
        return false;
    }
    // `$P$B` hashes take 2^13 rounds, whatever the request sends.
    match cookie.filter(|hash| hash.starts_with(b"$P$B")) {
        Some(hash) => !hyscribe_auth::check_password(&post.password, hash),
        None => true,
    }
}
