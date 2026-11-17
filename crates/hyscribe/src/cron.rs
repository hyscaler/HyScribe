// SPDX-License-Identifier: AGPL-3.0-or-later

//! WP-Cron in the service (ADR 0017): a task running `wp-cron.php` once a
//! minute in primary mode, and `hyscribe cron run` for the system's cron.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hyscribe_config::{Config, Mode};
use hyscribe_db::Writer;
use hyscribe_write::cron::{Ran, begun, microtime, wp_cron};

/// `WP_CRON_LOCK_TIMEOUT`: how often the task looks for due events.
pub(crate) const EVERY: Duration = Duration::from_secs(60);

/// Why nothing runs in shadow mode.
const SHADOW: &str = "cron: off in shadow mode, which writes nothing";

/// What the configuration says of cron, as the start logs it; `Err` when
/// a persistent object cache keeps PHP's lock from HyScribe.
pub(crate) fn settings(config: &Config, home: &Path) -> (Vec<String>, Result<Settings, String>) {
    let mut lines = Vec::new();
    if config.alternate_wp_cron {
        lines.push("cron: ALTERNATE_WP_CRON is ignored; HyScribe makes no loopback request".into());
    }
    let uploads = config.uploads_dir.clone();
    // An existing site's object cache sits in the `wp-content` above its uploads.
    let cache = uploads.as_ref().and_then(|dir| dir.parent());
    let cache = cache.map(|dir| dir.join("object-cache.php"));
    let off = cache.filter(|cache| cache.exists()).map(|cache| {
        format!(
            "cron: off, since {} is a persistent object cache that keeps PHP's cron lock \
             out of the database",
            cache.display()
        )
    });
    let settings = Settings {
        constants: config.constants,
        uploads: uploads.unwrap_or_else(|| home.join("uploads")),
    };
    (lines, off.map_or(Ok(settings), Err))
}

/// The service's cron task's settings, its log lines first: none when
/// [`settings`] says so, in shadow mode, or with `DISABLE_WP_CRON`.
pub(crate) fn service(config: &Config, home: &Path) -> (Vec<String>, Option<Settings>) {
    let (mut lines, settings) = settings(config, home);
    let disabled = "cron: DISABLE_WP_CRON is set; run ./hyscribe cron run from the system's cron";
    let settings = settings.and_then(|settings| match (config.mode, config.disable_wp_cron) {
        (Mode::Shadow, _) => Err(SHADOW.to_string()),
        (Mode::Primary, true) => Err(disabled.to_string()),
        (Mode::Primary, false) => Ok(settings),
    });
    lines.extend(settings.as_ref().err().cloned());
    (lines, settings.ok())
}

/// What a run needs besides the writer.
pub(crate) struct Settings {
    constants: hyscribe_config::Constants,
    /// The uploads folder, which holds the personal data exports.
    uploads: PathBuf,
}

/// One run at the time `clock` gives, its log lines written to `out`;
/// `failed` keeps the hooks whose jobs failed.
async fn once(
    writer: &Writer,
    settings: &Settings,
    clock: &dyn Fn() -> f64,
    failed: &mut Vec<hyscribe_write::cron::Job>,
    out: &mut dyn Write,
) {
    let run = (settings.constants, settings.uploads.as_path());
    let ran = wp_cron(&|| begun(writer.begin()), run, clock, failed).await;
    let lines = match ran {
        Ok(Ran::Idle) => Vec::new(),
        Ok(Ran::Locked) => vec!["cron: another process holds the lock; waiting".to_string()],
        Ok(Ran::Done { events, stolen }) => {
            let mut lines: Vec<String> = events
                .into_iter()
                .map(|(job, result)| match result {
                    Ok(()) => format!("cron: {} ran", job.hook()),
                    Err(err) => format!(
                        "cron: {} failed and waits for a restart; its events stay: {err:?}",
                        job.hook()
                    ),
                })
                .collect();
            if stolen {
                lines.push("cron: another process took the lock; stopped".into());
            }
            lines
        }
        Err(err) => vec![format!("cron: stopped by a database error: {err:?}")],
    };
    for line in lines {
        // Log lines are best effort, as the server's are.
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
}

/// The service's task: a run every `period`, the first at once, until
/// `stop` is notified; a run under way ends first.
pub(crate) async fn task(
    writer: &Writer,
    settings: &Settings,
    (period, stop): (Duration, &tokio::sync::Notify),
    out: &mut dyn Write,
) {
    let mut failed = Vec::new();
    let mut ticks = tokio::time::interval(period);
    // A run longer than the period is not followed by a burst of runs.
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticks.tick() => once(writer, settings, &microtime, &mut failed, out).await,
            () = stop.notified() => return,
        }
    }
}

/// `hyscribe cron run`: the due events once, then exit.
pub(crate) fn run(config: &Config, home: &Path, out: &mut dyn Write) -> Result<String, String> {
    let (lines, settings) = settings(config, home);
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    let settings = settings?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(crate::failed("cannot start the runtime".into()));
    runtime.and_then(|runtime| {
        runtime.block_on(async {
            let writer = Writer::open(&config.database, config.mode).await;
            let writer = writer.map_err(|err| format!("cannot open the writer: {err}"))?;
            let Some(writer) = writer else {
                return Err(SHADOW.to_string());
            };
            once(&writer, &settings, &microtime, &mut Vec::new(), out).await;
            Ok(String::new())
        })
    })
}

#[cfg(test)]
mod tests;
