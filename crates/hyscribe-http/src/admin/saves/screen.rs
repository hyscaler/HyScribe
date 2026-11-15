// SPDX-License-Identifier: AGPL-3.0-or-later

//! The screen state each user keeps: the user settings a page view
//! syncs with their cookie (`wp_user_settings()`, `set_user_setting()`),
//! the Screen Options form's `set_screen_options()` and the admin scripts'
//! saves through `admin-ajax.php`.

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_auth::caps::{post_type_exists, taxonomy_cap};
use hyscribe_db::{DbError, Write};
use hyscribe_kses::sanitize_key;
use hyscribe_phpser::{Key, Value, add_query_arg, addslashes, lookup, parse_str, remove_query_arg};
use hyscribe_write::Error;
use hyscribe_write::options::{Meta, update_metadata};

use super::{Saving, text};

/// `update_user_meta( $user, $key, $value )`, `value` as PHP passes it.
pub(in crate::admin) async fn update_user_meta(
    write: &mut Write<'_>,
    user: u64,
    key: &str,
    value: &Value,
) -> Result<(), DbError> {
    update_metadata(write, Meta::User, user, key, value, &Value::Null).await?;
    Ok(())
}

/// `update_user_option( $user, $name, $value )`: the site's own key.
pub(in crate::admin) async fn update_user_option(
    write: &mut Write<'_>,
    user: u64,
    name: &str,
    value: &Value,
) -> Result<(), DbError> {
    let key = format!("{}{name}", write.prefix());
    update_user_meta(write, user, &key, value).await
}

/// `text` without the bytes `preg_replace()` drops: all but ASCII letters,
/// digits and `kept`.
fn only(text: &[u8], kept: &[u8]) -> Vec<u8> {
    let keep = |b: &&u8| b.is_ascii_alphanumeric() || kept.contains(b);
    text.iter().filter(keep).copied().collect()
}

/// A user's settings as a page view finds them.
pub(in crate::admin) struct Found<'a> {
    /// `$_COOKIE['wp-settings-{ID}']`.
    pub cookie: Option<&'a [u8]>,
    /// `$_COOKIE['wp-settings-time-{ID}']`.
    pub time: Option<&'a [u8]>,
    /// `get_user_option( 'user-settings' )`.
    pub stored: Option<Value>,
    /// `get_user_option( 'user-settings-time' )`.
    pub saved: Option<Value>,
}

/// What a page view does with them: the options it writes and the
/// cookies it sets.
#[derive(Debug, Default, PartialEq)]
pub(in crate::admin) struct Synced {
    /// `set_user_setting( 'posts_list_mode', ... )`'s settings, saved now.
    pub mode: Option<Vec<u8>>,
    /// A newer cookie's settings, saved as five seconds ago.
    pub newer: Option<Vec<u8>>,
    /// The settings the cookies are set to, with the time now.
    pub cookie: Option<Vec<u8>>,
}

/// `get_all_user_settings()`: the cookie's when sent, else the option's.
fn all_settings(found: &Found) -> Value {
    let given = match found.cookie {
        Some(cookie) => {
            Some(only(cookie, b"=&_-")).filter(|c| c.iter().position(|b| *b == b'=') > Some(0))
        }
        None => found
            .stored
            .as_ref()
            .filter(|s| !s.is_empty())
            .and_then(Value::as_bytes)
            .map(<[u8]>::to_vec),
    };
    given.map_or(Value::Array(Vec::new()), |given| parse_str(&given))
}

/// `wp_set_all_user_settings()`'s value of `settings`.
fn joined(settings: &[(Key, Value)]) -> Vec<u8> {
    let pairs = settings.iter().filter_map(|(name, value)| {
        let name = only(&name.to_php_string(), b"_-");
        let value = only(&value.to_php_string(), b"_-");
        (!Value::String(name.clone()).is_empty()).then(|| [name, value].join(&b'='))
    });
    pairs.collect::<Vec<_>>().join(&b'&')
}

/// `set_user_setting( 'posts_list_mode', $mode )` for a list's `mode`
/// when one is given, then `wp_user_settings()`.
pub(in crate::admin) fn sync(found: &Found, mode: Option<&[u8]>, now: i64) -> Synced {
    let mut stored = found
        .stored
        .as_ref()
        .map(Value::to_php_string)
        .unwrap_or_default();
    let mut saved = found.saved.as_ref().map_or(0, Value::to_int);
    let mode = mode.map(|mode| {
        let mut settings = all_settings(found).as_array().unwrap_or_default().to_vec();
        let name = Key::String(b"posts_list_mode".to_vec());
        let mode = Value::String(
            if mode == b"excerpt" {
                &b"excerpt"[..]
            } else {
                b"list"
            }
            .to_vec(),
        );
        match settings.iter_mut().find(|(key, _)| *key == name) {
            Some((_, value)) => *value = mode,
            None => settings.push((name, mode)),
        }
        stored = joined(&settings);
        saved = now;
        stored.clone()
    });
    let mut synced = Synced {
        mode,
        ..Synced::default()
    };
    let time = found
        .time
        .unwrap_or_default()
        .iter()
        .filter(|b| b.is_ascii_digit());
    let time = Value::String(time.copied().collect()).to_int();
    match found.cookie.map(|cookie| only(cookie, b"=&_")) {
        Some(cookie) if cookie == stored => {}
        Some(cookie) if time > saved => synced.newer = Some(cookie),
        _ => synced.cookie = Some(stored),
    }
    synced
}

/// The options of `synced` for `user` at `now`: `mode`'s alone when `all` is false.
pub(in crate::admin) async fn save_settings(
    write: &mut Write<'_>,
    user: u64,
    synced: &Synced,
    (now, all): (i64, bool),
) -> Result<(), DbError> {
    let newer = synced
        .newer
        .as_ref()
        .filter(|_| all)
        .map(|newer| (newer, now - 5));
    for (settings, time) in synced.mode.iter().map(|mode| (mode, now)).chain(newer) {
        update_user_option(
            write,
            user,
            "user-settings",
            &Value::String(settings.clone()),
        )
        .await?;
        update_user_option(write, user, "user-settings-time", &Value::Int(time)).await?;
    }
    Ok(())
}

/// `$_POST[ $name ]`, slashed as PHP's is, items too.
fn slashed(asked: &Asked, name: &str) -> Option<Value> {
    fn slash(value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(addslashes(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|(k, v)| (k.clone(), slash(v))).collect())
            }
            value => value.clone(),
        }
    }
    lookup(asked.post, name).map(slash)
}

/// `explode( ',', $_POST[ $name ] )`, keyed from 0; an array is PHP's fatal error.
fn exploded(asked: &Asked, name: &str) -> Result<Vec<(Key, Value)>, Error> {
    let list = slashed(asked, name).unwrap_or_else(|| Value::String(Vec::new()));
    let Value::String(list) = list else {
        return Err(Error::Unported("a list given as an array"));
    };
    let items = list.split(|b| *b == b',').enumerate();
    Ok(items
        .map(|(n, item)| (Key::Int(n as i64), Value::String(item.to_vec())))
        .collect())
}

/// `$_POST['page']`, which the checks found `sanitize_key()` keeps.
fn page(asked: &Asked) -> String {
    text(
        &lookup(asked.post, "page")
            .map(Value::to_php_string)
            .unwrap_or_default(),
    )
}

/// `wp_die( 1 )`.
fn done() -> Result<Answer, super::Stop> {
    Ok(Answer::Page("1".into()))
}

/// `wp_ajax_closed_postboxes()`: the boxes closed and hidden on a screen.
pub(super) fn closed_postboxes<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        // A list not sent is empty, as the one item of an empty list is filtered out.
        let given = |name| -> Result<Vec<(Key, Value)>, Error> {
            let list = exploded(asked, name)?.into_iter();
            Ok(list.filter(|(_, item)| !item.is_empty()).collect())
        };
        let (closed, hidden) = (given("closed")?, given("hidden")?);
        let page = page(asked);
        update_user_meta(
            write,
            asked.user,
            &format!("closedpostboxes_{page}"),
            &Value::Array(closed),
        )
        .await?;
        // Postboxes that are always shown.
        let shown = [
            &b"submitdiv"[..],
            b"linksubmitdiv",
            b"manage-menu",
            b"create-menu",
        ];
        let hidden = hidden
            .into_iter()
            .filter(|(_, item)| !shown.contains(&&*item.to_php_string()));
        let hidden = Value::Array(hidden.collect());
        update_user_meta(write, asked.user, &format!("metaboxhidden_{page}"), &hidden).await?;
        done()
    })
}

/// `wp_ajax_hidden_columns()`: the columns hidden on a list.
pub(super) fn hidden_columns<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let given = lookup(asked.post, "hidden").is_some_and(|hidden| !hidden.is_empty());
        let hidden = if given {
            exploded(asked, "hidden")?
        } else {
            Vec::new()
        };
        let key = format!("manage{}columnshidden", page(asked));
        update_user_meta(write, asked.user, &key, &Value::Array(hidden)).await?;
        done()
    })
}

/// `wp_ajax_meta_box_order()`: the meta boxes' order and the screen's columns.
pub(super) fn meta_box_order<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let order = slashed(asked, "order").map(|order| match order {
            Value::Array(items) => Value::Array(items),
            item => Value::Array(vec![(Key::Int(0), item)]),
        });
        let columns = match lookup(asked.post, "page_columns") {
            Some(Value::String(auto)) if auto == b"auto" => Value::String(auto.clone()),
            Some(columns) => Value::Int(columns.to_int()),
            None => Value::String(b"auto".to_vec()),
        };
        let page = page(asked);
        if let Some(order) = order.filter(|order| !order.is_empty()) {
            update_user_meta(write, asked.user, &format!("meta-box-order_{page}"), &order).await?;
        }
        if !columns.is_empty() {
            update_user_meta(
                write,
                asked.user,
                &format!("screen_layout_{page}"),
                &columns,
            )
            .await?;
        }
        Ok(Answer::Page("{\"success\":true}".into()))
    })
}

/// The options `set_screen_options()` saves as a number of items per page.
const PER_PAGE: [&str; 8] = [
    "edit_per_page",
    "users_per_page",
    "edit_comments_per_page",
    "upload_per_page",
    "edit_tags_per_page",
    "plugins_per_page",
    "export_personal_data_requests_per_page",
    "remove_personal_data_requests_per_page",
];

/// `set_screen_options()`: a number of items per page, then the screen
/// again. What it does not save goes on to the screen's POST, not ported.
pub(super) fn options<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let form = lookup(asked.post, "wp_screen_options");
        let field = |name| form.and_then(|form| form.get_str(name));
        let option = field("option")
            .and_then(Value::as_bytes)
            .unwrap_or_default();
        let name = text(option);
        let kind = name.replace("edit_", "").replace("_per_page", "");
        let (key, mapped) = if taxonomy_cap(&kind, "edit_terms").is_some() {
            (name.clone(), "edit_tags_per_page".to_string())
        } else if post_type_exists(&kind) {
            (name.clone(), "edit_per_page".to_string())
        } else {
            (name.replace('-', "_"), name.replace('-', "_"))
        };
        let value = field("value").map_or(0, Value::to_int);
        // Other options go through filters, which a default install does not add.
        let saved = PER_PAGE.contains(&&*mapped) && (1..=999).contains(&value);
        if sanitize_key(option) != name || !saved {
            return Err(Error::Unported("a screen option not saved").into());
        }
        update_user_meta(write, asked.user, &key, &Value::Int(value)).await?;
        let referer = asked.wp_get_referer(&|location| {
            let host = hyscribe_kses::url_host(asked.home.as_bytes()).unwrap_or_default();
            crate::redirect::wp_validate_redirect(location, &host, b"")
        });
        let back = remove_query_arg(
            &[b"pagenum", b"apage", b"paged"],
            &referer.unwrap_or_else(|| asked.uri.to_vec()),
        );
        let back = match lookup(asked.post, "mode") {
            Some(Value::String(mode)) => {
                add_query_arg(&[(b"mode", Some(&addslashes(mode)))], &back)
            }
            Some(_) => return Err(Error::Unported("a mode given as an array").into()),
            None => back,
        };
        Ok(Answer::Redirect(text(&back)))
    })
}

#[cfg(test)]
pub(super) mod tests;
