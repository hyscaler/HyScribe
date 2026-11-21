// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Plugins screen's actions on one installed extension (MVP 11.3):
//! `plugins.php?action=activate` and `deactivate`, writing `active_plugins`
//! and `recently_activated` as PHP writes them, so a site switched back to
//! PHP WordPress finds the plugins the extensions stand in for active.
//! Activating approves what the extension asks for and runs its `activate`
//! export; deactivating runs its `deactivate` export and takes the approval
//! back, each where PHP fires the plugin's hooks.

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_db::Write;
use hyscribe_ext::installed::{self, Installed};
use hyscribe_ext::{Fault, Report, Runtime};
use hyscribe_kses::sanitize_key;
use hyscribe_model::hooks::do_action;
use hyscribe_phpser::{Key, Value, php_trim, urlencode};
use hyscribe_write::Error;
use hyscribe_write::options::{Autoload, get_option, update_option};

use super::{Acting, Saving, Stop, text};
use crate::extensions::{Acted, Run, files, log};

/// Why an activation stops when the extension's folder does not take it.
const UNAPPROVED: &str = "the approval could not be written";

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

/// The installed extension that stands in for plugin `file`, if any.
fn found<'s>(site: &'s dyn Acting, file: &str) -> Option<(&'s Runtime, Installed)> {
    let runtime = site.extensions()?;
    installed::find(runtime.plugins(), file).map(|found| (runtime, found))
}

/// `validate_plugin()` over the installed extensions: `wp_die()` of its
/// error for a path that climbs, or a plugin no extension stands in for.
fn validated<'s>(file: &str, site: &'s dyn Acting) -> Result<(&'s Runtime, Installed), Stop> {
    if hyscribe_admin::plugins::invalid_path(file) {
        return Err(Stop::Died(Answer::Died("Invalid plugin path.")));
    }
    found(site, file).ok_or(Stop::Died(Answer::Died("Plugin file does not exist.")))
}

/// `get_option( 'active_plugins', array() )`, which PHP walks as an array.
async fn active(write: &mut Write<'_>) -> Result<Vec<(Key, Value)>, Stop> {
    let stored = write.option_value("active_plugins").await?;
    hyscribe_admin::plugins::option_array(stored.as_deref())
        .ok_or(Error::Unported("an active_plugins that is not an array").into())
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

/// `activate_plugin()`'s hooks for `extension` before it is listed active:
/// `activate_plugin`, then its `activate` export where `activate_{$plugin}`
/// fires, with the extensions active in `current` before it, their database
/// calls in the action. None when it does not load or its export fails,
/// which leaves it inactive, as a fatal error leaves a plugin in PHP.
async fn activation(
    write: &mut Write<'_>,
    (runtime, extension): (&Runtime, &Installed),
    current: &[(Key, Value)],
    file: &str,
) -> Option<Acted> {
    let active = files(current);
    let made = Acted::new(|live| runtime.activating(&active, extension, live));
    let slug = extension.manifest.id.clone();
    let loaded = |fault: &Fault| {
        let (extension, hook, fault) = (slug.clone(), "load".into(), fault.clone());
        let report = Report {
            extension,
            hook,
            callback: None,
            fault,
        };
        log(&[report], &mut std::io::stderr());
    };
    let mut acted = made.inspect_err(loaded).ok()?;
    let file = file.to_string();
    let run: Run = Box::new(move |request| {
        do_action("activate_plugin", &[&file, &false]);
        request.index(&slug).map(|at| request.activate(at))
    });
    let activated = acted.run(write, run).await;
    if activated != Some(Ok(())) {
        log(&acted.reports(), &mut std::io::stderr());
        return None;
    }
    Some(acted)
}

/// Where `plugins.php` sends a plugin's activation that ended in a fatal
/// error: `activate_plugin()`'s first redirect, which no later one replaced.
fn failed(site: &dyn Acting, file: &str) -> Answer {
    let nonce = site.create_nonce(&format!("plugin-activation-error_{file}"));
    let plugin = text(&urlencode(file.as_bytes()));
    let url = format!("plugins.php?error=true&plugin={plugin}&_error_nonce={nonce}");
    Answer::Redirect(site.admin_url(&url))
}

/// `activate_plugin()` of `$_REQUEST['plugin']`, then the plugin taken off
/// `recently_activated` and its grants approved. Activating again enables
/// an extension its faults disabled. The redirects PHP sends to the
/// importers and Press This are not ported.
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
        let given = asked.plugin();
        let file = text(php_trim(given.as_bytes()));
        let (runtime, extension) = validated(&file, site)?;
        let cleared = hyscribe_ext::faults::clear(&extension.folder);
        cleared.map_err(|_| Error::Refused(UNAPPROVED))?;
        let mut current = active(write).await?;
        if !current.iter().any(|(_, entry)| *entry == named(&file)) {
            let found = (runtime, &extension);
            let Some(mut acted) = activation(write, found, &current, &file).await else {
                return Ok(failed(site, &file));
            };
            let mut files: Vec<Value> = current.into_iter().map(|(_, entry)| entry).collect();
            files.push(named(&file));
            files.sort_by_key(Value::to_php_string);
            current = (0..).map(Key::Int).zip(files).collect();
            update_option(write, "active_plugins", &Value::Array(current), None).await?;
            let file = file.clone();
            let run: Run = Box::new(move |_| {
                do_action("activated_plugin", &[&file, &false]);
                None
            });
            acted.run(write, run).await;
            log(&acted.reports(), &mut std::io::stderr());
        }
        let mut recent = recent(write).await?;
        recent.retain(|(key, _)| key.to_php_string() != given.as_bytes());
        let autoload = Some(Autoload::Off);
        update_option(write, "recently_activated", &Value::Array(recent), autoload).await?;
        installed::approve(&extension).map_err(|_| Error::Refused(UNAPPROVED))?;
        Ok(redirect(site, asked, "activate"))
    })
}

/// `deactivate_plugins()`'s hooks for `file` while it is still listed
/// active: `deactivate_plugin`, the `deactivate` export of the extension
/// standing in for it where `deactivate_{$plugin}` fires, then
/// `deactivated_plugin`, with the extensions active in `current`, their
/// database calls in the action. A failing export is logged, and the plugin
/// is deactivated all the same.
async fn deactivation(
    write: &mut Write<'_>,
    site: &dyn Acting,
    current: &[(Key, Value)],
    file: &str,
) {
    let Some(runtime) = site.extensions() else {
        return;
    };
    let active = files(current);
    let made = Acted::new(|live| {
        let (request, reports) = runtime.request(&active, None, live);
        log(&reports, &mut std::io::stderr());
        request.ok_or(())
    });
    let Ok(mut acted) = made else {
        return;
    };
    let slug = found(site, file).map(|(_, found)| found.manifest.id);
    let file = file.to_string();
    let run: Run = Box::new(move |request| {
        do_action("deactivate_plugin", &[&file, &false]);
        let at = slug.and_then(|slug| request.index(&slug));
        let _ = at.map(|at| request.deactivate(at));
        do_action("deactivated_plugin", &[&file, &false]);
        None
    });
    acted.run(write, run).await;
    log(&acted.reports(), &mut std::io::stderr());
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
        let file = asked.plugin();
        let mut current = active(write).await?;
        let at = current.iter().position(|(_, entry)| *entry == named(&file));
        if let Some(at) = at {
            deactivation(write, site, &current, &file).await;
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
        if let Some((_, found)) = found(site, &file) {
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
