// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::Arc;

use tokio::sync::Notify;

use super::*;
use crate::tests::test_registry;

fn config(values: &[(&str, &str)]) -> Config {
    let mut all = vec![("DB_NAME", "x"), ("DB_USER", "x")];
    all.extend_from_slice(values);
    let all = all.iter().map(|(k, v)| (k.to_string(), v.to_string()));
    hyscribe_config::from_map(&all.collect()).unwrap()
}

/// A client whose connections never open.
fn stuck() -> Client {
    struct Stuck;
    impl hyscribe_fetch::Net for Stuck {
        fn resolve<'a>(
            &'a self,
            _: &'a str,
            port: u16,
        ) -> hyscribe_fetch::Boxed<'a, Vec<std::net::SocketAddr>> {
            let address = std::net::SocketAddr::new([93, 184, 215, 14].into(), port);
            Box::pin(async move { Ok(vec![address]) })
        }

        fn dial(
            &self,
            _: std::net::SocketAddr,
        ) -> hyscribe_fetch::Boxed<'_, tokio::net::TcpStream> {
            Box::pin(std::future::pending())
        }
    }
    let roots = hyscribe_testkit::registry::roots();
    Client::new(roots, Arc::new(Stuck), Arc::new(()))
}

fn check(client: Client, base: &str, home: &Path) -> Check {
    Check {
        client,
        base: base.into(),
        home: home.to_path_buf(),
    }
}

/// The check runs only with all three keys, and the start says why not.
#[test]
fn the_configuration_decides_whether_the_registry_is_checked() {
    let home = Path::new("/srv/hyscribe");
    let registry = ("EXTENSION_REGISTRY", "https://pages.test/");
    let (on, outbound) = (("EXTENSION_UPDATE_CHECK", "on"), ("HTTP_OUTBOUND", "on"));
    let line = |values: &[(&str, &str)]| {
        let (line, check) = service(&config(values), home, stuck);
        (line, check.map(|check| (check.base, check.home)))
    };
    assert_eq!(line(&[registry, outbound]), (None, None));
    let off = |why: &str| Some(format!("update check: off, since {why}"));
    assert_eq!(line(&[on, registry]), (off("HTTP_OUTBOUND is off"), None));
    let unset = off("EXTENSION_REGISTRY is not set");
    assert_eq!(line(&[on, outbound]), (unset, None));
    let every = Some("update check: every 12 hours from https://pages.test/".to_string());
    let checked = Some(("https://pages.test/".to_string(), home.to_path_buf()));
    assert_eq!(line(&[on, outbound, registry]), (every, checked));
}

/// A log that asks the task to stop once a check has written its lines.
struct Stopping<'a> {
    out: Vec<u8>,
    stop: &'a Notify,
}

impl Write for Stopping<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.out.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stop.notify_one();
        Ok(())
    }
}

/// The first check runs at once, keeps the registry's files and logs what
/// `update --check` says; without a check the task ends at once.
#[tokio::test]
async fn the_task_checks_at_once_until_stopped() {
    let (_served, client) = test_registry();
    let home = tempfile::tempdir().unwrap();
    let check = check(client, "https://pages.test/", home.path());
    let stop = Notify::new();
    let mut out = Stopping {
        out: Vec::new(),
        stop: &stop,
    };
    let ran = task(Some(&check), (EVERY, &stop), &mut out);
    tokio::time::timeout(Duration::from_secs(60), ran)
        .await
        .unwrap();
    let logged = String::from_utf8(out.out).unwrap();
    assert_eq!(
        logged,
        "update check: Every plugin from the registry is up to date.\n"
    );
    let state = std::fs::read_to_string(home.path().join("registry/state")).unwrap();
    assert_eq!(state, "keys 1\nindex 1\n");
    let mut none = Vec::new();
    task(None, (EVERY, &stop), &mut none).await;
    assert!(none.is_empty());
}

/// A stop ends a check under way; a check is bounded in time, and a
/// failure is logged.
#[tokio::test]
async fn a_check_ends_on_stop_on_its_limit_and_on_failure() {
    let home = tempfile::tempdir().unwrap();
    let hanging = check(stuck(), "https://pages.test/", home.path());
    let stop = Notify::new();
    let mut out = Vec::new();
    let stopping = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stop.notify_one();
    };
    tokio::join!(task(Some(&hanging), (EVERY, &stop), &mut out), stopping);
    assert!(out.is_empty());
    once(&hanging, Duration::ZERO, &mut out).await;
    let bad = check(stuck(), "https://pages.test:22/", home.path());
    once(&bad, LIMIT, &mut out).await;
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "update check: stopped after 0 seconds\nupdate check: failed: https://pages.test:22/ is not an https: registry URL\n"
    );
    assert!(!home.path().join("registry").exists());
}
