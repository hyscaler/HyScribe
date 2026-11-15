// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Settings screens' saves: `options.php`'s, the Permalinks screen's
//! and the Privacy screen's new policy page, with the hooks of the options
//! they change. No mail is sent (MVP phase 8): neither the new admin
//! email's confirmation nor the change notice to the old one.

use hyscribe_admin::actions::{Asked, option_page};
use hyscribe_admin::menu::link;
use hyscribe_admin::posts::Answer;
use hyscribe_db::Write;
use hyscribe_phpser::{
    Key, Value, add_query_arg, addslashes, hash_equals, lookup, md5, php_trim, stripslashes,
};
use hyscribe_render::formatting::is_email;
use hyscribe_render::permalinks::home_path;
use hyscribe_write::Error;
use hyscribe_write::options::{
    Autoload, INVALID_ZONE, delete_option, get_option, is_zone, sanitize_option, set_transient,
    update_option,
};
use hyscribe_write::posts::{Context, PostArgs, wp_insert_post};

use super::Saving;
use crate::redirect::wp_validate_redirect;
use crate::rewrite::{Settings, rules};

/// `WP_Privacy_Policy_Content::get_default_content()`, `%s` the home URL.
const POLICY: &str = include_str!("privacy-policy.html");

/// `add_settings_error()`'s entries, in order.
#[derive(Default)]
struct Errors(Vec<Value>);

impl Errors {
    fn add(&mut self, setting: &str, code: &str, message: &str, kind: &str) {
        let field = |name: &str, value: &str| {
            let key = Key::String(name.as_bytes().to_vec());
            (key, Value::String(value.as_bytes().to_vec()))
        };
        self.0.push(Value::Array(vec![
            field("setting", setting),
            field("code", code),
            field("message", message),
            field("type", kind),
        ]));
    }

    /// `set_transient( 'settings_errors', get_settings_errors(), 30 )`.
    async fn keep(self, write: &mut Write<'_>) -> Result<(), Error> {
        let list = self.0.into_iter().enumerate();
        let list = list.map(|(i, error)| (Key::Int(i as i64), error));
        set_transient(write, "settings_errors", &Value::Array(list.collect()), 30).await?;
        Ok(())
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `home_url()` as the options are now.
async fn home_url(write: &mut Write<'_>) -> Result<String, Error> {
    Ok(text(&get_option(write, "home").await?.to_php_string()))
}

/// `admin_url( $path )` under hs-admin's names.
async fn admin_url(write: &mut Write<'_>, path: &str) -> Result<String, Error> {
    let site = get_option(write, "siteurl").await?.to_php_string();
    Ok(format!("{}/hs-admin/{}", text(&site), link(path)))
}

/// `options.php`'s lists of the options each page saves, on a single site.
async fn allowed(write: &mut Write<'_>, page: &[u8]) -> Result<Vec<&'static str>, Error> {
    let mut options = match page {
        b"general" => vec![
            "blogname",
            "blogdescription",
            "site_icon",
            "gmt_offset",
            "date_format",
            "time_format",
            "start_of_week",
            "timezone_string",
            "WPLANG",
            "new_admin_email",
        ],
        b"discussion" => vec![
            "default_pingback_flag",
            "default_ping_status",
            "default_comment_status",
            "comments_notify",
            "moderation_notify",
            "comment_moderation",
            "require_name_email",
            "comment_previously_approved",
            "comment_max_links",
            "moderation_keys",
            "disallowed_keys",
            "show_avatars",
            "avatar_rating",
            "avatar_default",
            "close_comments_for_old_posts",
            "close_comments_days_old",
            "thread_comments",
            "thread_comments_depth",
            "page_comments",
            "comments_per_page",
            "default_comments_page",
            "comment_order",
            "comment_registration",
            "show_comments_cookies_opt_in",
            "wp_notes_notify",
        ],
        b"media" => vec![
            "thumbnail_size_w",
            "thumbnail_size_h",
            "thumbnail_crop",
            "medium_size_w",
            "medium_size_h",
            "large_size_w",
            "large_size_h",
            "image_default_size",
            "image_default_align",
            "image_default_link_type",
            "uploads_use_yearmonth_folders",
        ],
        b"reading" => vec![
            "posts_per_page",
            "posts_per_rss",
            "rss_use_excerpt",
            "show_on_front",
            "page_on_front",
            "page_for_posts",
            "blog_public",
        ],
        b"writing" => vec![
            "default_category",
            "default_email_category",
            "default_link_category",
            "default_post_format",
            "mailserver_url",
            "mailserver_port",
            "mailserver_login",
            "mailserver_pass",
        ],
        // `misc` and `privacy`: their options are gone.
        _ => Vec::new(),
    };
    match page {
        b"reading" => {
            let charset = write
                .option_value("blog_charset")
                .await?
                .unwrap_or_default();
            if !charset.eq_ignore_ascii_case(b"utf-8") && !charset.eq_ignore_ascii_case(b"utf8") {
                return Err(Error::Unported("a charset other than UTF-8"));
            }
        }
        b"writing" => {
            let initial = get_option(write, "initial_db_version").await?;
            if initial.to_int() < 32453 {
                options.extend(["use_smilies", "use_balanceTags"]);
            }
            if write.option_value("blog_public").await?.as_deref() == Some(b"1") {
                options.push("ping_sites");
            }
        }
        b"general" => {
            let urls = ["siteurl", "home"].into_iter();
            options.extend(urls.filter(|url| write.configured(url).is_none()));
            options.extend(["users_can_register", "default_role"]);
        }
        _ => {}
    }
    Ok(options)
}

/// `$_POST[ $name ]` set to `value`.
fn set(post: &mut Vec<(Key, Value)>, name: &str, value: Value) {
    post.retain(|(key, _)| key.to_php_string() != name.as_bytes());
    post.push((Key::String(name.as_bytes().to_vec()), value));
}

/// What `options.php` changes in `$_POST` before saving the General page:
/// custom date and time formats, and a UTC offset given as a time zone.
async fn general(
    write: &mut Write<'_>,
    post: &mut Vec<(Key, Value)>,
    errors: &mut Errors,
) -> Result<(), Error> {
    for kind in ["date_format", "time_format"] {
        let custom = format!("{kind}_custom");
        let chosen = lookup(post, kind).and_then(Value::as_bytes);
        if let (Some(b"\\c\\u\\s\\t\\o\\m"), Some(custom)) = (chosen, lookup(post, &custom)) {
            set(post, kind, custom.clone());
        }
    }
    let Some(zone) = lookup(post, "timezone_string").cloned() else {
        return Ok(());
    };
    let utc = match &zone {
        Value::Array(_) if !zone.is_empty() => {
            return Err(Error::Unported("an array as the time zone"));
        }
        Value::String(zone) => zone
            .strip_prefix(b"UTC")
            .filter(|rest| rest.starts_with(b"+") || rest.starts_with(b"-")),
        _ => None,
    };
    if utc.is_some() {
        set(
            post,
            "gmt_offset",
            Value::String(without_utc(&zone.to_php_string())),
        );
        set(post, "timezone_string", Value::String(Vec::new()));
    } else if !zone.as_bytes().is_some_and(is_zone) {
        let current = get_option(write, "timezone_string").await?;
        if current.is_empty() {
            let offset = get_option(write, "gmt_offset").await?;
            set(post, "gmt_offset", offset);
            set(post, "timezone_string", Value::String(Vec::new()));
        } else {
            set(post, "timezone_string", current);
        }
        errors.add("general", "settings_updated", INVALID_ZONE, "error");
    }
    Ok(())
}

/// `preg_replace( '/UTC\+?/', '', $zone )`.
fn without_utc(zone: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = zone;
    while let Some((&first, tail)) = rest.split_first() {
        match rest.strip_prefix(b"UTC") {
            Some(after) => rest = after.strip_prefix(b"+").unwrap_or(after),
            None => {
                out.push(first);
                rest = tail;
            }
        }
    }
    out
}

/// `update_option( $option, $value )`, sanitized, and the hooks the admin
/// runs on its change: a settings error instead when the value is refused.
async fn save(
    write: &mut Write<'_>,
    option: &str,
    value: Value,
    errors: &mut Errors,
) -> Result<(), Error> {
    let old = get_option(write, option).await?;
    let sanitized = sanitize_option(write, option, value).await?;
    if let Some(message) = sanitized.error {
        // The stored value is saved again: nothing changes.
        errors.add(option, &format!("invalid_{option}"), message, "error");
        return Ok(());
    }
    let value = sanitized.value;
    if !update_option(write, option, &value, None).await? {
        return Ok(());
    }
    let added = old == Value::Bool(false);
    match option {
        "new_admin_email" => new_admin_email(write, &value).await,
        _ if added => Ok(()),
        "home" => {
            https_migration(write, &old, &value).await?;
            flush_rewrite_rules(write).await
        }
        "siteurl" | "page_on_front" => flush_rewrite_rules(write).await,
        "WPLANG" => Err(Error::Unported("a change of the site language")),
        _ => Ok(()),
    }
}

/// `update_option_new_admin_email()`: the key that confirms a new admin
/// email, kept with it until then.
async fn new_admin_email(write: &mut Write<'_>, email: &Value) -> Result<(), Error> {
    let bytes = email.to_php_string();
    if get_option(write, "admin_email").await? == *email || !is_email(&bytes) {
        return Ok(());
    }
    // `wp_rand()`'s number, as random letters: the key is a hash of it.
    let random = hyscribe_auth::generate_password(12, false).map_err(Error::Random)?;
    let time = crate::now();
    let hash = md5(&[&bytes[..], time.to_string().as_bytes(), random.as_bytes()].concat());
    let pending = Value::Array(vec![
        (
            Key::String(b"hash".to_vec()),
            Value::String(hash.into_bytes()),
        ),
        (Key::String(b"newemail".to_vec()), email.clone()),
    ]);
    update_option(write, "adminhash", &pending, Some(Autoload::Off)).await?;
    Ok(())
}

/// `wp_update_https_migration_required()` for a new home URL.
async fn https_migration(write: &mut Write<'_>, old: &Value, new: &Value) -> Result<(), Error> {
    let untrailed = |value: &Value| {
        let url = text(&value.to_php_string());
        url.trim_end_matches(['/', '\\']).to_string()
    };
    if untrailed(old) != untrailed(new).replace("https://", "http://") {
        delete_option(write, "https_migration_required").await?;
        return Ok(());
    }
    let fresh = get_option(write, "fresh_site").await?;
    let required = Value::Bool(fresh.is_empty());
    update_option(write, "https_migration_required", &required, None).await?;
    Ok(())
}

/// `flush_rewrite_rules()`'s `rewrite_rules` option. The `.htaccess` it
/// also writes on Apache is not written.
async fn flush_rewrite_rules(write: &mut Write<'_>) -> Result<(), Error> {
    let mut option = async |name| -> Result<String, Error> {
        Ok(text(&get_option(write, name).await?.to_php_string()))
    };
    let structure = option("permalink_structure").await?;
    let category_base = option("category_base").await?;
    let tag_base = option("tag_base").await?;
    let page_on_front = option("page_on_front").await?;
    let home = option("home").await?;
    let settings = Settings {
        structure: &structure,
        category_base: &category_base,
        tag_base: &tag_base,
        page_on_front: &page_on_front,
        home_path: home_path(&home),
    };
    let rules = rules(&settings).into_iter().map(|(regex, query)| {
        (
            Key::String(regex.into_bytes()),
            Value::String(query.into_bytes()),
        )
    });
    let rules: Vec<(Key, Value)> = rules.collect();
    let value = if structure.is_empty() {
        Value::String(Vec::new())
    } else {
        Value::Array(rules)
    };
    update_option(write, "rewrite_rules", &value, None).await?;
    Ok(())
}

/// `$_POST[ $option ]` as `options.php` saves it: trimmed before it is
/// unslashed, unless an array; `null` when not sent.
fn posted(post: &[(Key, Value)], option: &str) -> Value {
    match lookup(post, option) {
        None => Value::Null,
        Some(items @ Value::Array(_)) => items.clone(),
        Some(value) => {
            let slashed = addslashes(&value.to_php_string());
            Value::String(stripslashes(php_trim(&slashed)))
        }
    }
}

/// `options.php`'s save of a Settings page, sent back with
/// `settings-updated` and its notices kept in the `settings_errors` transient.
pub(super) fn update<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let page = option_page(asked);
        if page == b"options" {
            return Err(Error::Unported("the All Settings screen's save").into());
        }
        let mut post = asked.post.to_vec();
        let mut errors = Errors::default();
        let options = allowed(write, &page).await?;
        if page == b"general" {
            general(write, &mut post, &mut errors).await?;
        }
        if options.is_empty() {
            errors.add(
                "general",
                "settings_updated",
                "Settings save failed.",
                "error",
            );
        }
        for option in options {
            let value = posted(&post, option);
            save(write, option, value, &mut errors).await?;
        }
        if errors.0.is_empty() {
            errors.add("general", "settings_updated", "Settings saved.", "success");
        }
        errors.keep(write).await?;
        let home = home_url(write).await?;
        let host = hyscribe_kses::url_host(home.as_bytes()).unwrap_or_default();
        let referer = asked.wp_get_referer(&|referer| wp_validate_redirect(referer, &host, b""));
        let back = referer.unwrap_or_else(|| addslashes(asked.uri));
        let to = add_query_arg(&[(b"settings-updated", Some(b"true"))], &back);
        Ok(Answer::Redirect(text(&to)))
    })
}

/// `options.php`'s confirmation of a pending admin email change by its key.
pub(super) fn adminhash<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let pending = get_option(write, "adminhash").await?;
        let mut redirect = "options-general.php?updated=false";
        if let Value::Array(_) = pending {
            // `hash_equals()` of anything but two strings is a fatal error.
            let known = pending.get_str("hash").and_then(Value::as_bytes);
            let given = lookup(asked.get, "adminhash").and_then(Value::as_bytes);
            let (Some(known), Some(given)) = (known, given) else {
                return Err(Error::Unported("a confirmation key that is not a string").into());
            };
            let email = pending.get_str("newemail").cloned().unwrap_or(Value::Null);
            if hash_equals(known, &addslashes(given)) && !email.is_empty() {
                let mut errors = Errors::default();
                save(write, "admin_email", email, &mut errors).await?;
                delete_option(write, "adminhash").await?;
                delete_option(write, "new_admin_email").await?;
                redirect = "options-general.php?updated=true";
            }
        }
        Ok(Answer::Redirect(admin_url(write, redirect).await?))
    })
}

/// `options.php`'s dismissal of a pending admin email change.
pub(super) fn dismiss<'a>(write: &'a mut Write<'_>, _: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        delete_option(write, "adminhash").await?;
        delete_option(write, "new_admin_email").await?;
        let to = admin_url(write, "options-general.php?updated=true").await?;
        Ok(Answer::Redirect(to))
    })
}

/// A base or structure as `options-permalink.php` takes it from the slashed
/// `$_POST`: without `#`, from a single `/`, unless empty.
fn permalink_field(value: &Value) -> Result<Value, Error> {
    let Some(given) = value.as_bytes() else {
        return Err(Error::Unported("an array as a permalink setting"));
    };
    let given = addslashes(given);
    if Value::String(given.clone()).is_empty() {
        return Ok(Value::String(given));
    }
    let mut out = vec![b'/'];
    for b in given.into_iter().filter(|&b| b != b'#') {
        if !(b == b'/' && out.last() == Some(&b'/')) {
            out.push(b);
        }
    }
    Ok(Value::String(out))
}

/// The Permalinks screen's save of its structure and bases, sent back with
/// `settings-updated`. With no structure, PHP goes on to show the screen,
/// which refreshes the rewrite rules: not ported.
pub(super) fn permalinks<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let field = |name| lookup(asked.post, name).filter(|value| **value != Value::Null);
        let Some(given) = field("permalink_structure") else {
            return Err(Error::Unported("a permalink base saved alone").into());
        };
        let mut errors = Errors::default();
        let chosen = field("selection").filter(|chosen| chosen.as_bytes() != Some(b"custom"));
        let structure = permalink_field(chosen.unwrap_or(given))?;
        let sanitized = sanitize_option(write, "permalink_structure", structure).await?;
        if let Some(message) = sanitized.error {
            errors.add(
                "permalink_structure",
                "invalid_permalink_structure",
                message,
                "error",
            );
        }
        // `WP_Rewrite::set_permalink_structure()` and the bases' setters.
        let mut changes = vec![("permalink_structure", sanitized.value)];
        for base in ["category_base", "tag_base"] {
            if let Some(value) = field(base) {
                changes.push((base, permalink_field(value)?));
            }
        }
        for (option, value) in changes {
            if get_option(write, option).await? != value {
                save(write, option, value, &mut errors).await?;
            }
        }
        if errors.0.is_empty() {
            let message = "Permalink structure updated.";
            errors.add("general", "settings_updated", message, "success");
        }
        errors.keep(write).await?;
        let to = admin_url(write, "options-permalink.php?settings-updated=true").await?;
        Ok(Answer::Redirect(to))
    })
}

/// The Privacy screen's new draft of the suggested policy, which becomes
/// the site's policy page, opened in the editor.
pub(super) fn create_privacy_page<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
) -> Saving<'a> {
    Box::pin(async move {
        let home = home_url(write).await?;
        let site = get_option(write, "siteurl").await?.to_php_string();
        let ctx = Context {
            user: asked.user,
            can_publish: false,
            home: home.clone().into_bytes(),
            upload_host: hyscribe_render::link::host(&site),
            now: crate::now(),
        };
        let page = PostArgs {
            title: b"Privacy Policy".to_vec(),
            status: "draft".into(),
            post_type: "page".into(),
            content: POLICY.replace("%s", &home).into_bytes(),
            ..PostArgs::default()
        };
        let id = wp_insert_post(write, &ctx, page).await?;
        let option = Value::Int(i64::try_from(id).unwrap_or_default());
        update_option(write, "wp_page_for_privacy_policy", &option, None).await?;
        let to = admin_url(write, &format!("post.php?post={id}&action=edit")).await?;
        Ok(Answer::Redirect(to))
    })
}

#[cfg(test)]
mod tests;
