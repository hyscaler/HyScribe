// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runs over `wordpress_saves`, loaded afresh for each test, since each run
//! commits its lock and its events as PHP's `wp-cron.php` does.

use super::*;
use crate::cron::{Event, args, wp_next_scheduled, wp_schedule_event, wp_schedule_single_event};
use hyscribe_config::{Database, Mode};
use hyscribe_db::{Write, Writer};

/// The lock PHP left in the default install fixture, and its time.
const PHP_LOCK: &str = "1793088495.8851230144500732421875";
const PHP_TIME: f64 = 1_793_088_495.885_123;
/// The time of the fixture's first events, among them two HyScribe runs.
const FIRST: i64 = 1_793_088_495;

/// One test at a time over `wordpress_saves`, which each reloads.
static SAVES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The default install afresh in `wordpress_saves`, after `sql`.
async fn saves(sql: &[&str]) -> Writer {
    let db = Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = <hyscribe_db::Connection as sqlx::Connection>::connect_with(&options)
        .await
        .unwrap();
    for statement in sql {
        let query = sqlx::query(sqlx::AssertSqlSafe(*statement));
        query.execute(&mut conn).await.unwrap();
    }
    Writer::open(&db, Mode::Primary).await.unwrap().unwrap()
}

/// Runs `change` as one committed action.
async fn act(writer: &Writer, change: impl AsyncFnOnce(&mut Write<'_>)) {
    let mut write = writer.begin().await.unwrap();
    change(&mut write).await;
    write.commit().await.unwrap();
}

async fn option(writer: &Writer, name: &str) -> Option<Vec<u8>> {
    writer
        .begin()
        .await
        .unwrap()
        .option_value(name)
        .await
        .unwrap()
}

async fn next(writer: &Writer, hook: &str) -> Option<i64> {
    let mut write = writer.begin().await.unwrap();
    let none = Value::Array(Vec::new());
    wp_next_scheduled(&mut write, hook, &none).await.unwrap()
}

/// A run at `time` with nothing failed before.
async fn at(writer: &Writer, time: f64, uploads: &Path) -> Ran {
    wp_cron(
        &|| begun(writer.begin()),
        (Constants::default(), uploads),
        &|| time,
        &mut Vec::new(),
    )
    .await
    .unwrap()
}

/// The hooks a run went through, with whether each worked, and whether
/// the lock was stolen.
fn done(ran: Ran) -> (Vec<(&'static str, bool)>, bool) {
    let Ran::Done { events, stolen } = ran else {
        panic!("{ran:?}");
    };
    let events = events.iter().map(|(job, r)| (job.hook(), r.is_ok()));
    (events.collect(), stolen)
}

#[test]
fn the_lock_is_written_as_php_formats_microtime() {
    // `sprintf( '%.22F', $time )` in PHP 8.4.
    assert_eq!(lock_value(PHP_TIME), PHP_LOCK.as_bytes());
    for (time, php) in [
        (1_763_300_000.123_456, "1763300000.1234560012817382812500"),
        (1_763_300_000.5, "1763300000.5000000000000000000000"),
        (1_000_000_000.000_001, "1000000000.0000009536743164062500"),
    ] {
        assert_eq!(lock_value(time), php.as_bytes());
    }
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    let now = since.unwrap().as_secs_f64();
    let micro = microtime();
    assert!((micro - now).abs() < 1.0, "{micro} {now}");
    assert_ne!(micro.fract(), 0.0);
}

#[tokio::test]
async fn a_lock_php_holds_waits_and_a_stale_one_is_taken() {
    let _saves = SAVES.lock().await;
    let uploads = tempfile_dir();
    let writer = saves(&[]).await;
    let cron = option(&writer, "cron").await;
    // PHP's own lock, 59 seconds old: nothing runs or changes.
    assert!(matches!(
        at(&writer, PHP_TIME + 59.0, &uploads).await,
        Ran::Locked
    ));
    assert_eq!(option(&writer, "cron").await, cron);
    assert_eq!(
        option(&writer, "_transient_doing_cron").await.unwrap(),
        PHP_LOCK.as_bytes()
    );
    // Sixty seconds old: HyScribe's two due hooks run, PHP's stay.
    let time = PHP_TIME + 60.0;
    let ran = at(&writer, time, &uploads).await;
    let hooks = vec![
        ("wp_privacy_delete_old_export_files", true),
        ("wp_privacy_personal_data_cleanup_requests", true),
    ];
    assert_eq!(done(ran), (hooks, false));
    assert_eq!(option(&writer, "_transient_doing_cron").await, None);
    let now = time.floor() as i64;
    let exports = next(&writer, "wp_privacy_delete_old_export_files").await;
    assert_eq!(exports, Some(now + 3_600 - (now - FIRST)));
    let requests = next(&writer, "wp_privacy_personal_data_cleanup_requests").await;
    assert_eq!(requests, Some(FIRST + 86_400));
    assert_eq!(
        next(&writer, "recovery_mode_clean_expired_keys").await,
        Some(FIRST)
    );
    // Nothing of HyScribe's is due any more, so no lock is taken.
    assert!(matches!(at(&writer, time, &uploads).await, Ran::Idle));
    assert_eq!(option(&writer, "_transient_doing_cron").await, None);
}

/// A folder for the exports that does not exist.
fn tempfile_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("hyscribe-cron-no-uploads")
}

#[tokio::test]
async fn the_lock_s_bounds_are_php_s() {
    let _saves = SAVES.lock().await;
    let uploads = tempfile_dir();
    let time = 1_793_100_000.0;
    let lock = |value: &str| {
        format!(
            "UPDATE wp_options SET option_value = '{value}' WHERE option_name = '_transient_doing_cron'"
        )
    };
    // Sixty seconds old exactly, or more than ten minutes ahead, is no lock.
    for free in ["1793099940", "1793100600.5", "", "x"] {
        let writer = saves(&[&lock(free)]).await;
        assert!(
            matches!(at(&writer, time, &uploads).await, Ran::Done { .. }),
            "{free}"
        );
    }
    for held in ["1793099940.5", "1793100600", "1793100000"] {
        let writer = saves(&[&lock(held)]).await;
        assert!(
            matches!(at(&writer, time, &uploads).await, Ran::Locked),
            "{held}"
        );
    }
    // No lock row at all.
    let writer =
        saves(&["DELETE FROM wp_options WHERE option_name = '_transient_doing_cron'"]).await;
    assert!(matches!(
        at(&writer, time, &uploads).await,
        Ran::Done { .. }
    ));
}

#[tokio::test]
async fn a_run_stops_when_its_lock_is_taken() {
    let _saves = SAVES.lock().await;
    let uploads = tempfile_dir();
    // An expired timeout PHP left: deleting expired transients takes the lock too.
    let writer = saves(&[
        "DELETE FROM wp_options WHERE option_name = '_transient_doing_cron'",
        "INSERT INTO wp_options (option_name, option_value, autoload) \
         VALUES ('_transient_timeout_doing_cron', '1', 'off')",
    ])
    .await;
    act(&writer, async |w| {
        let none = Value::Array(Vec::new());
        let hook = "delete_expired_transients";
        assert!(
            wp_schedule_event(w, FIRST - 1, "daily", hook, &none)
                .await
                .unwrap()
        );
    })
    .await;
    let ran = at(&writer, PHP_TIME + 60.0, &uploads).await;
    assert_eq!(done(ran), (vec![("delete_expired_transients", true)], true));
    assert_eq!(option(&writer, "_transient_doing_cron").await, None);
    // The events after it were left.
    assert_eq!(
        next(&writer, "wp_privacy_delete_old_export_files").await,
        Some(FIRST)
    );
}

#[tokio::test]
async fn a_failing_job_keeps_its_event_and_its_hook_waits() {
    let _saves = SAVES.lock().await;
    let uploads = tempfile_dir();
    // Two scheduled posts of a type HyScribe does not port.
    let writer = saves(&[
        "UPDATE wp_posts SET post_type = 'product', post_status = 'future' WHERE ID IN (1, 2)",
        "DELETE FROM wp_options WHERE option_name = '_transient_doing_cron'",
    ])
    .await;
    let now = FIRST + 100;
    act(&writer, async |w| {
        for (post, at) in [(1, FIRST - 2), (2, FIRST - 1)] {
            wp_schedule_single_event(w, (now, at), "publish_future_post", Some(post))
                .await
                .unwrap();
        }
    })
    .await;
    let mut failed = Vec::new();
    let settings = (Constants::default(), uploads.as_path());
    let clock = || now as f64;
    let ran = wp_cron(&|| begun(writer.begin()), settings, &clock, &mut failed)
        .await
        .unwrap();
    let Ran::Done { events, stolen } = &ran else {
        panic!("{ran:?}");
    };
    assert!(!stolen);
    assert!(matches!(
        events[0],
        (Job::PublishFuturePost, Err(crate::Error::Unported(_)))
    ));
    // The second post's event waits with the hook; the others ran.
    let hooks: Vec<_> = events.iter().map(|(job, _)| job.hook()).collect();
    assert_eq!(
        hooks,
        [
            "publish_future_post",
            "wp_privacy_delete_old_export_files",
            "wp_privacy_personal_data_cleanup_requests"
        ]
    );
    assert_eq!(failed, [Job::PublishFuturePost]);
    let mut write = writer.begin().await.unwrap();
    let left = crate::cron::wp_get_ready_cron_jobs(&mut write, now)
        .await
        .unwrap();
    let posts: Vec<_> = left.iter().map(|e: &Event| e.args.clone()).collect();
    assert_eq!(posts, [args(Some(1)), args(Some(2))]);
    drop(write);
    // A later run leaves the hook, so nothing is due for it.
    let ran = wp_cron(&|| begun(writer.begin()), settings, &clock, &mut failed)
        .await
        .unwrap();
    assert!(matches!(ran, Ran::Idle));
}

/// A lock taken after the last event's check is left to its new owner.
#[tokio::test]
async fn a_lock_taken_at_the_end_is_left() {
    let _saves = SAVES.lock().await;
    let uploads = tempfile_dir();
    let writer =
        saves(&["DELETE FROM wp_options WHERE option_name = '_transient_doing_cron'"]).await;
    let count = std::cell::Cell::new(0);
    // The run's seventh action is its last: due, lock, and two events' run and check.
    let begin = || {
        let (n, writer) = (count.replace(count.get() + 1), &writer);
        begun(async move {
            if n == 6 {
                let mut write = writer.begin().await?;
                write
                    .update_option("_transient_doing_cron", b"theirs")
                    .await?;
                write.commit().await?;
            }
            writer.begin().await
        })
    };
    let settings = (Constants::default(), uploads.as_path());
    let ran = wp_cron(&begin, settings, &|| PHP_TIME + 60.0, &mut Vec::new()).await;
    assert!(!done(ran.unwrap()).1);
    assert_eq!(count.get(), 7);
    let lock = option(&writer, "_transient_doing_cron").await;
    assert_eq!(lock.as_deref(), Some(&b"theirs"[..]));
}

/// Each action of a run fails in turn, first as it begins, then at each of
/// its statements: the run stops with the error, or a job records it.
#[tokio::test]
async fn database_errors_stop_a_run() {
    let _saves = SAVES.lock().await;
    let uploads = tempfile_dir();
    let missing = Database {
        name: "wordpress_saves".into(),
        prefix: hyscribe_config::Prefix::new("missing_").unwrap(),
        ..hyscribe_testkit::fixture_database()
    };
    let refused = Writer::open(&missing, Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let no_lock = "DELETE FROM wp_options WHERE option_name = '_transient_doing_cron'";
    // A run begins seven actions: the eighth "failure" is never reached.
    let mut started = 0;
    for action in 0..8 {
        let mut worked = false;
        for budget in std::iter::once(None).chain((0..16).map(Some)) {
            let writer = saves(&[no_lock]).await;
            let count = std::cell::Cell::new(0);
            let begin = || {
                let n = count.replace(count.get() + 1);
                let (writer, refused) = (&writer, &refused);
                begun(async move {
                    match budget {
                        None if n == action => refused.begin().await,
                        Some(budget) if n == action => {
                            let mut write = writer.begin().await?;
                            write.fail_after(budget);
                            Ok(write)
                        }
                        _ => writer.begin().await,
                    }
                })
            };
            let settings = (Constants::default(), uploads.as_path());
            let ran = wp_cron(&begin, settings, &|| PHP_TIME + 60.0, &mut Vec::new()).await;
            started = count.get();
            match ran {
                Err(err) => assert!(matches!(err, crate::Error::Db(_)), "{err:?}"),
                Ok(_) => {
                    worked = true;
                    break;
                }
            }
        }
        assert!(worked, "action {action}");
    }
    assert_eq!(started, 7);
}
