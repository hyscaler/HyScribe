// SPDX-License-Identifier: AGPL-3.0-or-later

//! `sanitize_option()` for the options the Settings screens save, with
//! `wpdb::strip_invalid_text_for_column()` for the `options` table's
//! `utf8mb4` value column. An array where PHP stops on a `TypeError` is
//! not ported.

use hyscribe_db::Write;
use hyscribe_kses::{esc_html, esc_url_raw, strip_tags};
use hyscribe_phpser::{Key, Maybe, Value, absint, maybe_unserialize, php_trim};
use hyscribe_render::formatting::{is_email, sanitize_email};
use hyscribe_render::kses::wp_kses_data;

use crate::Error;

use super::get_option;

/// `timezone_identifiers_list( DateTimeZone::ALL_WITH_BC )` of the PHP
/// oracle (`reference/settings-saves/sanitize-golden.php`).
const ZONES: &str = include_str!("zones.txt");

const INVALID_EMAIL: &str = "The email address entered did not appear to be a valid email address. Please enter a valid email address.";

/// The message of an invalid time zone, also `options.php`'s own.
pub const INVALID_ZONE: &str =
    "The timezone you have entered is not valid. Please select a valid timezone.";

/// What `sanitize_option()` returns, and the settings error it adds.
#[derive(Debug, PartialEq)]
pub struct Sanitized {
    /// The value to save: the stored one when there is an error.
    pub value: Value,
    /// The message of the `invalid_{$option}` settings error, if any.
    pub error: Option<&'static str>,
}

/// Whether `zone` is a time zone PHP knows, as spelled.
pub fn is_zone(zone: &[u8]) -> bool {
    ZONES.lines().any(|known| known.as_bytes() == zone)
}

/// The bytes of a valid UTF-8 text kept, as the regex `strip_invalid_text()`
/// runs for a `utf8mb4` column keeps them.
fn strip_invalid(text: &[u8]) -> Vec<u8> {
    text.utf8_chunks()
        .flat_map(|chunk| chunk.valid().as_bytes())
        .copied()
        .collect()
}

/// A value the string functions take; an array is a fatal error to them.
fn text(value: &Value) -> Result<Vec<u8>, Error> {
    match value {
        Value::Array(_) => Err(Error::Unported("an array for a text setting")),
        value => Ok(value.to_php_string()),
    }
}

/// `array_filter()` without a callback over strings: `''` and `'0'` go.
fn kept(item: &&[u8]) -> bool {
    !item.is_empty() && *item != b"0"
}

/// Lines trimmed, with the empty ones left out.
fn lines(text: &[u8]) -> Vec<&[u8]> {
    text.split(|&b| b == b'\n')
        .map(php_trim)
        .filter(kept)
        .collect()
}

/// `preg_match( '#http(s?)://(.+)#i', $value )`.
fn has_url(text: &[u8]) -> bool {
    let lower = text.to_ascii_lowercase();
    ["http://", "https://"].iter().any(|scheme| {
        lower.windows(scheme.len()).enumerate().any(|(at, window)| {
            window == scheme.as_bytes() && lower.get(at + scheme.len()).is_some_and(|&b| b != b'\n')
        })
    })
}

/// `preg_match( '/%[^\/%]+%/', $value )`: a structure tag.
fn has_tag(text: &[u8]) -> bool {
    let parts: Vec<&[u8]> = text.split(|&b| b == b'%').skip(1).collect();
    // Each part but the last is closed by a `%`.
    let closed = parts.split_last().map_or(&[][..], |(_, closed)| closed);
    closed
        .iter()
        .any(|tag| !tag.is_empty() && !tag.contains(&b'/'))
}

/// Whether `role` is one of the roles stored in `{prefix}user_roles`.
async fn role_exists(write: &mut Write<'_>, role: &[u8]) -> Result<bool, Error> {
    let name = format!("{}user_roles", write.prefix());
    let roles = write.option_value(&name).await?.unwrap_or_default();
    let role = Key::from_php_string(role.to_vec());
    Ok(matches!(maybe_unserialize(&roles), Maybe::Value(roles) if roles.get(&role).is_some()))
}

/// `sanitize_option( $option, $value )` for `option`, with the error it adds.
pub async fn sanitize_option(
    write: &mut Write<'_>,
    option: &str,
    value: Value,
) -> Result<Sanitized, Error> {
    let string = |bytes: Vec<u8>| Value::String(bytes);
    let mut error = None;
    let value = match option {
        "admin_email" | "new_admin_email" => {
            let email = sanitize_email(&strip_invalid(&text(&value)?));
            if !is_email(&email) {
                error = Some(INVALID_EMAIL);
            }
            string(email)
        }
        "thumbnail_size_w"
        | "thumbnail_size_h"
        | "medium_size_w"
        | "medium_size_h"
        | "medium_large_size_w"
        | "medium_large_size_h"
        | "large_size_w"
        | "large_size_h"
        | "mailserver_port"
        | "comment_max_links"
        | "page_on_front"
        | "page_for_posts"
        | "rss_excerpt_length"
        | "default_category"
        | "default_email_category"
        | "default_link_category"
        | "close_comments_days_old"
        | "comments_per_page"
        | "thread_comments_depth"
        | "users_can_register"
        | "start_of_week"
        | "site_icon"
        | "fileupload_maxk" => Value::Int(absint(&value) as i64),
        "posts_per_page" | "posts_per_rss" => match value.to_int() {
            0 => Value::Int(1),
            n if n < -1 => Value::Int(n.abs()),
            n => Value::Int(n),
        },
        "default_ping_status" | "default_comment_status" => match value {
            Value::String(status) if status.is_empty() || status == b"0" => {
                string(b"closed".to_vec())
            }
            value => value,
        },
        "blogdescription" | "blogname" => {
            // An array is `Array` to `esc_html()`.
            let original = value.to_php_string();
            let stripped = strip_invalid(&original);
            // PHP tries again with emoji as entities, which is not ported.
            if stripped != original && !stripped.is_ascii() {
                return Err(Error::Unported("invalid UTF-8 beside other text"));
            }
            string(esc_html(&stripped).into_bytes())
        }
        "blog_public" => match value {
            Value::Null => Value::Int(1),
            value => Value::Int(value.to_int()),
        },
        "date_format" | "time_format" | "mailserver_url" | "mailserver_login"
        | "mailserver_pass" | "upload_path" => {
            string(wp_kses_data(&strip_tags(&strip_invalid(&text(&value)?))))
        }
        "ping_sites" => {
            let sites = text(&value)?;
            let urls: Vec<Vec<u8>> = lines(&sites).into_iter().map(esc_url_raw).collect();
            let urls: Vec<&[u8]> = urls.iter().map(Vec::as_slice).filter(kept).collect();
            string(urls.join(&b'\n'))
        }
        "gmt_offset" if value.is_numeric() => {
            let offset = value.to_php_string();
            string(
                offset
                    .into_iter()
                    .filter(|b| b"0123456789:.-".contains(b))
                    .collect(),
            )
        }
        "gmt_offset" => string(Vec::new()),
        "siteurl" | "home" => {
            let url = strip_invalid(&text(&value)?);
            if !has_url(&url) {
                error = Some(match option {
                    "siteurl" => {
                        "The WordPress address you entered did not appear to be a valid URL. Please enter a valid URL."
                    }
                    _ => {
                        "The Site address you entered did not appear to be a valid URL. Please enter a valid URL."
                    }
                });
            }
            string(esc_url_raw(&url))
        }
        // No translation is installed, nor can be (`DISALLOW_FILE_MODS`).
        "WPLANG" if !value.is_empty() => get_option(write, option).await?,
        "timezone_string" => {
            let known = value.as_bytes().is_some_and(is_zone);
            if !known && !value.is_empty() {
                error = Some(INVALID_ZONE);
            }
            value
        }
        "permalink_structure" | "category_base" | "tag_base" => {
            let url = esc_url_raw(&strip_invalid(&text(&value)?));
            let url = String::from_utf8_lossy(&url).replace("http://", "");
            if option == "permalink_structure" && !url.is_empty() && !has_tag(url.as_bytes()) {
                error = Some(
                    "A structure tag is required when using custom permalinks. <a href=\"https://wordpress.org/documentation/article/customize-permalinks/#choosing-your-permalink-structure\">Learn more</a>",
                );
            }
            string(url.into_bytes())
        }
        "default_role" => {
            let role = text(&value)?;
            if !role_exists(write, &role).await? && role_exists(write, b"subscriber").await? {
                string(b"subscriber".to_vec())
            } else {
                value
            }
        }
        "moderation_keys" | "disallowed_keys" => {
            let keys = strip_invalid(&text(&value)?);
            let mut unique: Vec<&[u8]> = Vec::new();
            for key in lines(&keys) {
                if !unique.contains(&key) {
                    unique.push(key);
                }
            }
            string(unique.join(&b'\n'))
        }
        _ => value,
    };
    let value = match error {
        Some(_) => get_option(write, option).await?,
        None => value,
    };
    Ok(Sanitized { value, error })
}

#[cfg(test)]
mod tests;
