// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

const SITE: &str = "http://127.0.0.1:8081";

/// The oracle's site: its URLs filled in, the request's nonces left as
/// their placeholders, as the golden keeps them.
struct Oracle;

impl Urls for Oracle {
    fn site_url(&self) -> String {
        SITE.into()
    }
    fn admin_url(&self, path: &str) -> String {
        format!("{SITE}/wp-admin/{path}")
    }
    fn fill(&self, text: &str) -> String {
        text.replace("%rest_url%", &format!("{SITE}/index.php?rest_route=/"))
            .replace("%admin_url%", &format!("{SITE}/wp-admin/"))
            .replace("\\\"%admin_path%", "\\\"/wp-admin/")
            .replace("\"%admin_path%", "\"/wp-admin/")
            .replace("%site_url%", SITE)
    }
}

/// The placeholders `reference/admin-assets/golden.php` writes over PHP's.
fn placeholders(text: &str) -> String {
    text.replace(&format!("{SITE}/index.php?rest_route=/"), "%rest_url%")
        .replace(&format!("{SITE}/wp-admin/"), "%admin_url%")
        .replace("\"/wp-admin/", "\"%admin_path%")
        .replace(SITE, "%site_url%")
}

/// Every case of `reference/admin-assets/golden.sh`.
#[test]
fn assets_print_as_php_prints_them() {
    let golden = include_str!("../../tests/golden/assets.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let mut assets = Assets::new(f[0] == "1");
        for handle in f[1].split(',').filter(|h| !h.is_empty()) {
            assets.enqueue_style(handle);
        }
        for handle in f[2].split(',').filter(|h| !h.is_empty()) {
            assets.enqueue_script(handle);
        }
        let mut out = assets.print_admin_styles(&Oracle);
        out.extend(assets.print_head_scripts(&Oracle));
        out.extend_from_slice(b"<!-- footer -->\n");
        out.extend(assets.print_footer_scripts(&Oracle));
        let ours = placeholders(&String::from_utf8(out).unwrap());
        let php = String::from_utf8(unhex(f[3])).unwrap();
        if ours != php {
            let at = ours
                .bytes()
                .zip(php.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(php.len()));
            let near = |text: &str| {
                text.get(at.saturating_sub(80)..(at + 120).min(text.len()))
                    .unwrap_or_default()
                    .to_string()
            };
            failures.push(format!(
                "{} {} {}\n  php:  {:?}\n  ours: {:?}",
                f[0],
                f[1],
                f[2],
                near(&php),
                near(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        golden.lines().count(),
        failures.join("\n")
    );
}

/// What the printer leaves out of `WP_Styles` and `WP_Scripts`, because
/// core registers nothing that needs it.
#[test]
fn the_registry_needs_no_more_than_the_printer_ports() {
    let extra = |item: &Item, key: &str| item.extra.get_str(key).is_some_and(|v| !v.is_empty());
    for ((scripts, handle), item) in registry() {
        for dep in &item.deps {
            assert!(
                registry().contains_key(&(*scripts, dep.clone())),
                "{handle} needs {dep}"
            );
        }
        if *scripts {
            let delayed = extra(item, "strategy") || extra(item, "fetchpriority");
            let dependents = registry()
                .iter()
                .any(|((s, _), i)| *s && i.deps.contains(handle));
            assert!(!delayed || !dependents && !extra(item, "after"), "{handle}");
            assert!(
                item.src.is_some() || !extra(item, "before") && !extra(item, "after"),
                "{handle}"
            );
            // WP_Scripts never concatenates these.
            let l10n = item
                .src
                .as_deref()
                .is_some_and(|src| src.starts_with("/wp-includes/js/l10n"));
            assert!(!l10n, "{handle}");
        } else {
            assert!(!extra(item, "alt") && !extra(item, "title"), "{handle}");
            assert!(item.src.is_none() || !extra(item, "after"), "{handle}");
        }
    }
    // Every style prints with media 'all'.
    let data = json_decode(REGISTRY).unwrap_or(Value::Null);
    let styles = data
        .get_str("styles")
        .and_then(Value::as_array)
        .unwrap_or_default();
    assert!(!styles.is_empty());
    for (handle, item) in styles {
        assert_eq!(item.get_str("args"), Some(&Value::Null), "{handle:?}");
    }
}
