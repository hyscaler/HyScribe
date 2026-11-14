// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_allow_comment()`: the duplicate and flood checks, then
//! `wp_check_comment_data()`'s approval from the discussion options
//! (`check_comment()` and `wp_check_comment_disallowed_list()`). PHP passes
//! them slashed data, which the option checks match as it is.

use hyscribe_kses::{str_contains, wp_strip_all_tags};
use hyscribe_phpser::{Value, addslashes, is_empty_string, php_trim, stripslashes};
use hyscribe_render::date::{Zone, format_unix, stored_unix};
use hyscribe_render::front_comment_text;

use super::{Commenter, Context, MYSQL, NewComment, option, step};
use crate::steps::Steps;

/// Why `wp_new_comment()` refused a comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// `comment_duplicate`: the same comment is there already.
    Duplicate,
    /// `comment_flood`: less than 15 seconds after the commenter's last.
    Flood,
}

/// `wp_allow_comment()`: the approval (`0`, `1` or `trash`), or why the
/// comment is refused.
pub(super) async fn allow_comment(
    steps: &mut Steps<'_, '_>,
    context: Context<'_>,
    comment: &NewComment,
    commenter: Commenter,
) -> Result<String, Refused> {
    let email = Some(&comment.author_email[..]).filter(|email| !is_empty_string(email));
    let (post, author, content) = (
        (comment.post_id, comment.parent),
        &comment.author,
        &comment.content,
    );
    if step!(steps, duplicate_comment(post, author, email, content)).is_some() {
        return Err(Refused::Duplicate);
    }
    if !(commenter.manage_options || commenter.moderate_comments) {
        let since = format_unix(MYSQL, context.now - 3600, Zone::Utc);
        let user = (comment.user_id, &comment.author_ip[..]);
        let email = addslashes(&comment.author_email);
        if let Some(last) = step!(steps, last_comment_date(&since, user, &email)) {
            let new = comment.date_gmt.as_deref().unwrap_or_default();
            if stored_unix(new, Zone::Utc) - stored_unix(&last, Zone::Utc) < 15 {
                return Err(Refused::Flood);
            }
        }
    }
    Ok(check_comment_data(steps, context, comment, commenter).await)
}

/// `wp_check_comment_data()`: the post's author and moderators are
/// approved; anyone else's comment is checked against the discussion
/// options, and goes to the trash if it has a disallowed word.
pub(super) async fn check_comment_data(
    steps: &mut Steps<'_, '_>,
    context: Context<'_>,
    comment: &NewComment,
    commenter: Commenter,
) -> String {
    if comment.user_id != 0 {
        let post_author = step!(steps, post_author(comment.post_id));
        if commenter.moderate_comments || post_author == Some(comment.user_id) {
            return "1".into();
        }
    }
    let fields = Slashed {
        author: addslashes(&comment.author),
        email: addslashes(&comment.author_email),
        url: addslashes(&comment.author_url),
        content: addslashes(&comment.content),
        ip: addslashes(&comment.author_ip),
        agent: addslashes(&comment.agent),
    };
    let approved = check_comment(steps, context, &fields, &comment.comment_type).await;
    let disallowed = option(steps, "disallowed_keys").await;
    let words = php_trim(&disallowed);
    if !words.is_empty() {
        let without_html = wp_strip_all_tags(&fields.content, false);
        let subjects = [
            &fields.author[..],
            &fields.email,
            &fields.url,
            &fields.content,
            &without_html,
            &fields.ip,
            &fields.agent,
        ];
        if has_word(words, &subjects) {
            return "trash".into();
        }
    }
    if approved { "1" } else { "0" }.into()
}

/// The fields `check_comment()` and the disallowed list read, slashed.
struct Slashed {
    author: Vec<u8>,
    email: Vec<u8>,
    url: Vec<u8>,
    content: Vec<u8>,
    ip: Vec<u8>,
    agent: Vec<u8>,
}

/// `check_comment()`: whether the comment may be approved without a
/// moderator. A pingback is always held (gap: PHP approves one from a
/// published post of the site).
async fn check_comment(
    steps: &mut Steps<'_, '_>,
    context: Context<'_>,
    fields: &Slashed,
    comment_type: &str,
) -> bool {
    if option(steps, "comment_moderation").await == b"1" {
        return false;
    }
    // Smiley images get no site URL (gap: a key naming it is not found there).
    let use_smilies = step!(steps, option_value("use_smilies"));
    let options = |name: &str| use_smilies.clone().filter(|_| name == "use_smilies");
    let text = front_comment_text(&options, context.home, &fields.content);
    let max_links = option(steps, "comment_max_links").await;
    if !is_empty_string(&max_links) && links(&text) >= Value::String(max_links).to_int() {
        return false;
    }
    let keys = option(steps, "moderation_keys").await;
    let keys = php_trim(&keys);
    let subjects = [
        &fields.author[..],
        &fields.email,
        &fields.url,
        &text,
        &fields.ip,
        &fields.agent,
    ];
    if !is_empty_string(keys) && has_word(keys, &subjects) {
        return false;
    }
    if option(steps, "comment_previously_approved").await != b"1" {
        return true;
    }
    let named = !fields.author.is_empty() && !fields.email.is_empty();
    if !named || matches!(comment_type, "trackback" | "pingback") {
        return false;
    }
    let email = stripslashes(&fields.email);
    let email = php_trim(&email);
    let user = match is_empty_string(email) {
        true => None,
        false => step!(steps, user_id_by_email(email)),
    };
    let (author, email) = (&fields.author, &fields.email);
    let approved_before = match user {
        Some(user) => step!(steps, user_has_approved_comment(user)),
        None => step!(steps, author_has_approved_comment(author, email)),
    };
    approved_before && (is_empty_string(keys) || !str_contains(&fields.email, keys))
}

/// `preg_match_all( '/<a [^>]*href/i', $text )`.
fn links(text: &[u8]) -> i64 {
    let mut count = 0;
    let mut from = 0;
    while let Some(start) = text
        .get(from..)
        .and_then(|rest| rest.windows(3).position(|w| w.eq_ignore_ascii_case(b"<a ")))
        .map(|i| from + i + 3)
    {
        let tag = text.get(start..).unwrap_or_default();
        let end = tag.iter().position(|&b| b == b'>').unwrap_or(tag.len());
        let tag = tag.get(..end).unwrap_or_default();
        match tag
            .windows(4)
            .rposition(|w| w.eq_ignore_ascii_case(b"href"))
        {
            Some(href) => {
                count += 1;
                from = start + href + 4;
            }
            None => from = start + end,
        }
    }
    count
}

/// Whether a line of `words` is in one of `subjects`, as `#$word#iu`
/// matches: as a whole, in any case. Empty lines and `0` are skipped; a
/// word or subject that is not UTF-8 never matches, as PCRE refuses it.
fn has_word(words: &[u8], subjects: &[&[u8]]) -> bool {
    let subjects: Vec<Vec<char>> = subjects
        .iter()
        .filter_map(|subject| std::str::from_utf8(subject).ok())
        .map(|subject| subject.chars().map(fold).collect())
        .collect();
    words.split(|&b| b == b'\n').map(php_trim).any(|word| {
        let Some(word) = std::str::from_utf8(word)
            .ok()
            .filter(|word| !is_empty_string(word.as_bytes()))
        else {
            return false;
        };
        let word: Vec<char> = word.chars().map(fold).collect();
        subjects
            .iter()
            .any(|subject| subject.windows(word.len()).any(|w| w == word))
    })
}

/// A character as PCRE's caseless UTF matching compares it: its upper
/// case's lower case, where each is one character, but the dotless `ı`,
/// which PCRE matches with nothing else.
fn fold(c: char) -> char {
    if c == 'ı' {
        return c;
    }
    let upper = single(c.to_uppercase()).unwrap_or(c);
    single(upper.to_lowercase()).unwrap_or(upper)
}

/// The only character of `chars`, if it has one.
fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let first = chars.next();
    chars.next().is_none().then_some(first).flatten()
}

#[cfg(test)]
mod tests;
