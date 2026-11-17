// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runs over `wordpress_saves`, whose default install has two of
//! HyScribe's events due and PHP's own lock from its last run.

use super::*;
use crate::tests::{SAVES, saves_with};

/// The lock PHP left in the fixture, as a time.
const PHP_LOCK: f64 = 1_793_088_495.885_123;

fn primary(extra: &[(&str, &str)]) -> Config {
    let db = hyscribe_testkit::fixture_database();
    let port = db.port.to_string();
    let mut values: std::collections::HashMap<String, String> = [
        ("DB_HOST", db.host.as_str()),
        ("DB_PORT", &port),
        ("DB_NAME", "wordpress_saves"),
        ("DB_USER", &db.user),
        ("DB_PASSWORD", db.password.expose()),
        ("MODE", "primary"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    values.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    hyscribe_config::from_map(&values).unwrap()
}

#[test]
fn the_configuration_decides_whether_cron_runs() {
    let home = Path::new("/srv/hyscribe");
    let (lines, plain) = settings(&primary(&[]), home);
    assert!(lines.is_empty());
    let plain = plain.unwrap();
    assert_eq!(plain.uploads, home.join("uploads"));
    assert_eq!(plain.constants, hyscribe_config::Constants::default());
    // An object cache beside an existing site's uploads.
    let site = tempfile::tempdir().unwrap();
    let uploads = site.path().join("uploads");
    let config = primary(&[("UPLOADS_DIR", uploads.to_str().unwrap())]);
    assert_eq!(settings(&config, home).1.unwrap().uploads, uploads);
    std::fs::write(site.path().join("object-cache.php"), "<?php").unwrap();
    let cache = format!(
        "cron: off, since {}/object-cache.php is a persistent object cache that keeps \
         PHP's cron lock out of the database",
        site.path().display()
    );
    assert_eq!(settings_of(&config).1, Err(cache));
    let (lines, got) = settings_of(&primary(&[("ALTERNATE_WP_CRON", "true")]));
    let ignored = "cron: ALTERNATE_WP_CRON is ignored; HyScribe makes no loopback request";
    assert_eq!((lines, got.is_ok()), (vec![ignored.to_string()], true));
    // The service's task also stops for DISABLE_WP_CRON, which `cron run` ignores.
    let disabled = primary(&[("DISABLE_WP_CRON", "true")]);
    assert!(settings(&disabled, home).1.is_ok());
    let (lines, task) = service(&disabled, home);
    let line = "cron: DISABLE_WP_CRON is set; run ./hyscribe cron run from the system's cron";
    assert_eq!((lines, task.is_none()), (vec![line.to_string()], true));
    let shadow = primary(&[("MODE", "shadow"), ("DISABLE_WP_CRON", "true")]);
    let (lines, task) = service(&shadow, home);
    assert_eq!(lines, [SHADOW]);
    assert!(task.is_none());
    assert!(service(&primary(&[]), home).1.is_some());
}

/// [`settings`] with the settings left out.
fn settings_of(config: &Config) -> (Vec<String>, Result<(), String>) {
    let (lines, settings) = settings(config, Path::new("/"));
    (lines, settings.map(drop))
}

/// Runs `test` with a writer over `wordpress_saves` after `sql`.
fn with_saves(sql: &[&str], test: impl AsyncFnOnce(&Writer, &Settings)) {
    saves_with(&sql.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let config = primary(&[]);
    let settings = settings(&config, Path::new("/nonexistent")).1.unwrap();
    runtime.block_on(async {
        let writer = Writer::open(&config.database, Mode::Primary).await;
        test(&writer.unwrap().unwrap(), &settings).await;
    });
}

/// The log of one run at `time` with `failed` hooks.
async fn logged(writer: &Writer, settings: &Settings, time: f64) -> String {
    let mut out = Vec::new();
    once(writer, settings, &|| time, &mut Vec::new(), &mut out).await;
    String::from_utf8(out).unwrap()
}

#[test]
fn each_run_is_logged() {
    let _saves = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    with_saves(&[], async |writer, settings| {
        // Before any event is due, nothing; PHP's lock held, a wait.
        assert_eq!(logged(writer, settings, 1_000.0).await, "");
        let locked = logged(writer, settings, PHP_LOCK + 1.0).await;
        assert_eq!(locked, "cron: another process holds the lock; waiting\n");
        let ran = logged(writer, settings, PHP_LOCK + 61.0).await;
        assert_eq!(
            ran,
            "cron: wp_privacy_delete_old_export_files ran\n\
             cron: wp_privacy_personal_data_cleanup_requests ran\n"
        );
    });
    // A job HyScribe does not port, and a lock another process takes.
    let failing = [
        "UPDATE wp_posts SET post_type = 'product', post_status = 'future' WHERE ID = 1",
        "DELETE FROM wp_options WHERE option_name = '_transient_doing_cron'",
        "INSERT INTO wp_options (option_name, option_value, autoload) \
         VALUES ('_transient_timeout_doing_cron', '1', 'off')",
    ];
    with_saves(&failing, async |writer, settings| {
        let mut write = writer.begin().await.unwrap();
        let none = hyscribe_phpser::Value::Array(Vec::new());
        let post = hyscribe_phpser::Value::Array(vec![(
            hyscribe_phpser::Key::Int(0),
            hyscribe_phpser::Value::Int(1),
        )]);
        let schedule = hyscribe_write::cron::wp_schedule_event;
        let first = 1_793_088_000;
        schedule(&mut write, first, "weekly", "publish_future_post", &post)
            .await
            .unwrap();
        let hook = "delete_expired_transients";
        schedule(&mut write, first + 1, "daily", hook, &none)
            .await
            .unwrap();
        write.commit().await.unwrap();
        let log = logged(writer, settings, PHP_LOCK + 61.0).await;
        assert!(
            log.starts_with(
                "cron: publish_future_post failed and waits for a restart; its events stay: \
                 Unported(\"a post type other than post and page\")\n\
                 cron: delete_expired_transients ran\n\
                 cron: another process took the lock; stopped\n"
            ),
            "{log}"
        );
    });
    let broken = ["UPDATE wp_options SET option_value = 'x' WHERE option_name = 'db_version'"];
    with_saves(&broken, async |writer, settings| {
        let log = logged(writer, settings, PHP_LOCK + 61.0).await;
        assert!(
            log.starts_with("cron: stopped by a database error: Db("),
            "{log}"
        );
    });
}

#[test]
fn the_task_runs_until_it_is_stopped() {
    let _saves = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    with_saves(&[], async |writer, settings| {
        let stop = tokio::sync::Notify::new();
        let mut out = Vec::new();
        let stopping = async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            stop.notify_one();
        };
        let period = Duration::from_millis(10);
        tokio::join!(task(writer, settings, (period, &stop), &mut out), stopping);
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.starts_with("cron: wp_privacy_delete_old_export_files ran\n"),
            "{out}"
        );
    });
}
