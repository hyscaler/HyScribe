// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Plugins screen's actions on one installed extension (MVP 11.3):
//! `plugins.php?action=activate` and `deactivate`, writing `active_plugins`
//! and `recently_activated` as PHP writes them, so a site switched back to
//! PHP WordPress finds the plugins the extensions stand in for active.
//! Activating approves what the extension asks for; deactivating takes the
//! approval back.

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_db::Write;
use hyscribe_ext::installed::{self, Installed};
use hyscribe_kses::sanitize_key;
use hyscribe_phpser::{Key, Value, php_trim, urlencode};
use hyscribe_write::Error;
use hyscribe_write::options::{Autoload, get_option, update_option};

use super::{Acting, Saving, Stop, text};

/// `get_pagenum()`'s page, `$status` and `$s` as `plugins.php` puts them
/// in its redirects.
fn listed(asked: &Asked) -> String {
    let field = |name| asked.request(name).map(Value::to_php_string);
    let status =
        field("plugin_status").map_or_else(|| "all".into(), |status| sanitize_key(&status));
    let page = field("paged").map_or(1, |page| Value::String(page).to_int().unsigned_abs().max(1));
    let s = text(&urlencode(&field("s").unwrap_or_default()));
    format!("plugin_status={status}&paged={page}&s={s}")
}

/// `$_REQUEST['plugin']`, unslashed.
fn plugin(asked: &Asked) -> String {
    let plugin = asked.request("plugin").map(Value::to_php_string);
    text(&plugin.unwrap_or_default())
}

/// `validate_plugin()` over the installed extensions: `wp_die()` of its
/// error for a path that climbs, or a plugin no extension stands in for.
fn validated(file: &str, site: &dyn Acting) -> Result<Installed, Stop> {
    if hyscribe_admin::plugins::invalid_path(file) {
        return Err(Stop::Died(Answer::Died("Invalid plugin path.")));
    }
    let found = site
        .plugins()
        .and_then(|plugins| installed::find(plugins, file));
    found.ok_or(Stop::Died(Answer::Died("Plugin file does not exist.")))
}

/// `get_option( 'active_plugins', array() )`, which PHP walks as an array.
async fn active(write: &mut Write<'_>) -> Result<Vec<(Key, Value)>, Stop> {
    match get_option(write, "active_plugins").await? {
        Value::Bool(false) => Ok(Vec::new()),
        Value::Array(active) => Ok(active),
        _ => Err(Error::Unported("an active_plugins that is not an array").into()),
    }
}

/// `(array) get_option( 'recently_activated' )`.
async fn recent(write: &mut Write<'_>) -> Result<Vec<(Key, Value)>, Stop> {
    Ok(match get_option(write, "recently_activated").await? {
        Value::Array(recent) => recent,
        value => vec![(Key::Int(0), value)],
    })
}

fn named(file: &str) -> Value {
    Value::String(file.as_bytes().to_vec())
}

/// The redirect `plugins.php` sends once done, `done` naming the action.
fn redirect(site: &dyn Acting, asked: &Asked, done: &str) -> Answer {
    Answer::Redirect(site.admin_url(&format!("plugins.php?{done}=true&{}", listed(asked))))
}

/// `activate_plugin()` of `$_REQUEST['plugin']`, then the plugin taken off
/// `recently_activated` and its grants approved. The redirects PHP sends to
/// the importers and Press This are not ported.
pub(super) fn activate<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        if asked
            .get
            .iter()
            .any(|(key, _)| key.to_php_string() == b"from")
        {
            return Err(Error::Unported("an activation from an importer").into());
        }
        let given = plugin(asked);
        let file = text(php_trim(given.as_bytes()));
        let found = validated(&file, site)?;
        let mut current = active(write).await?;
        if !current.iter().any(|(_, entry)| *entry == named(&file)) {
            let mut files: Vec<Value> = current.into_iter().map(|(_, entry)| entry).collect();
            files.push(named(&file));
            files.sort_by_key(Value::to_php_string);
            current = (0..).map(Key::Int).zip(files).collect();
            update_option(write, "active_plugins", &Value::Array(current), None).await?;
        }
        let mut recent = recent(write).await?;
        recent.retain(|(key, _)| key.to_php_string() != given.as_bytes());
        let autoload = Some(Autoload::Off);
        update_option(write, "recently_activated", &Value::Array(recent), autoload).await?;
        let approved =
            installed::approve(&found).and_then(|()| hyscribe_ext::faults::clear(&found.folder));
        approved.map_err(|_| Error::Refused("the approval could not be written"))?;
        Ok(redirect(site, asked, "activate"))
    })
}

/// `deactivate_plugins()` of `$_REQUEST['plugin']`, then the plugin put
/// first in `recently_activated` at the time of the request, and an
/// installed extension's approval taken back.
pub(super) fn deactivate<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let file = plugin(asked);
        let mut current = active(write).await?;
        let at = current.iter().position(|(_, entry)| *entry == named(&file));
        if let Some(at) = at {
            current.remove(at);
            update_option(write, "active_plugins", &Value::Array(current), None).await?;
        }
        let mut recent = recent(write).await?;
        recent.retain(|(key, _)| key.to_php_string() != file.as_bytes());
        recent.insert(
            0,
            (
                Key::String(file.clone().into_bytes()),
                Value::Int(asked.now),
            ),
        );
        update_option(
            write,
            "recently_activated",
            &Value::Array(recent),
            Some(Autoload::Off),
        )
        .await?;
        let found = site
            .plugins()
            .and_then(|plugins| installed::find(plugins, &file));
        if let Some(found) = found {
            installed::revoke(&found)
                .map_err(|_| Error::Refused("the approval could not be removed"))?;
        }
        Ok(redirect(site, asked, "deactivate"))
    })
}

/// What a view of the Plugins screen writes, as `prepare_items()` does:
/// `recently_activated` with the plugins of the last week alone. One the
/// screen does not read is left as it is.
pub(in crate::admin) async fn keep_recent(write: &mut Write<'_>, now: i64) -> Result<(), Error> {
    let stored = write.option_value("recently_activated").await?;
    let kept = hyscribe_admin::plugins::recently_activated(stored.as_deref(), now);
    if let Some(kept) = kept {
        let autoload = Some(Autoload::Off);
        update_option(write, "recently_activated", &Value::Array(kept), autoload).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
