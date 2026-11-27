// SPDX-License-Identifier: AGPL-3.0-or-later

//! The service's update check (ADR 0024): with `EXTENSION_UPDATE_CHECK`,
//! `HTTP_OUTBOUND` and `EXTENSION_REGISTRY`, a task fetching the registry's
//! key list and index every 12 hours, as `hyscribe plugin update --check`
//! does: kept in `registry/`, revocations applied, nothing installed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hyscribe_config::Config;
use hyscribe_fetch::Client;
use hyscribe_registry::install::{self, Allow, Site, Update};
use hyscribe_registry::remote::Remote;

use crate::signed;

/// How often the registry is checked, 12 hours, as `wp_update_plugins()` runs.
pub(crate) const EVERY: Duration = Duration::from_secs(43_200);

/// The longest a check may take: its five fetches of 30 seconds at most.
const LIMIT: Duration = Duration::from_secs(300);

/// What a check needs.
pub(crate) struct Check {
    client: Client,
    base: String,
    home: PathBuf,
}

/// The check the configuration asks for, with the line the start logs:
/// none unless `EXTENSION_UPDATE_CHECK` is on, `HTTP_OUTBOUND` is on and
/// `EXTENSION_REGISTRY` is set.
pub(crate) fn service(
    config: &Config,
    home: &Path,
    client: impl FnOnce() -> Client,
) -> (Option<String>, Option<Check>) {
    if !config.extension_update_check {
        return (None, None);
    }
    let off = |why| Some(format!("update check: off, since {why}"));
    match (config.http_outbound, &config.extension_registry) {
        (false, _) => (off("HTTP_OUTBOUND is off"), None),
        (true, None) => (off("EXTENSION_REGISTRY is not set"), None),
        (true, Some(base)) => {
            let line = format!("update check: every 12 hours from {base}");
            let check = Check {
                client: client(),
                base: base.clone(),
                home: home.to_path_buf(),
            };
            (Some(line), Some(check))
        }
    }
}

/// One check, its lines written to `out`, within `limit`.
async fn once(check: &Check, limit: Duration, out: &mut dyn Write) {
    let site = Site {
        home: &check.home,
        roots: &signed::roots(),
        now: signed::now(),
    };
    let checked = async {
        let remote = Remote::new(&check.client, &check.base)?;
        install::update(&remote, &site, Update::Check, Allow::default()).await
    };
    let text = match tokio::time::timeout(limit, checked).await {
        Ok(Ok((text, _))) => text,
        Ok(Err(why)) => format!("failed: {why}\n"),
        Err(_) => format!("stopped after {} seconds\n", limit.as_secs()),
    };
    for line in text.lines() {
        // Log lines are best effort, as the server's are.
        let _ = writeln!(out, "update check: {line}");
    }
    let _ = out.flush();
}

/// The service's task: a check every `period`, the first at once, until
/// `stop` is notified, which also ends a check under way (its files are
/// written in one step after its last fetch); none without a check.
pub(crate) async fn task(
    check: Option<&Check>,
    (period, stop): (Duration, &tokio::sync::Notify),
    out: &mut dyn Write,
) {
    let Some(check) = check else { return };
    let mut ticks = tokio::time::interval(period);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticks.tick() => {}
            () = stop.notified() => return,
        }
        tokio::select! {
            () = once(check, LIMIT, out) => {}
            () = stop.notified() => return,
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
