// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::admin_bar::Browser;
use crate::media::attachment::{AttachmentArgs, attachment_edit};
use crate::tests::{BROWSER, Role, SITE, first_difference, request};

/// The answer of `screen` in `variant` to `role`, as the golden keeps it.
fn answer(variant: &str, role: &str, screen: &str) -> Option<String> {
    let site = Role(Box::leak(format!("{variant}:{role}").into_boxed_str()));
    let (file, query) = screen.split_once('?').unwrap_or((screen, ""));
    let answer = match file {
        "post-new.php" => post_new(&site, query)?,
        _ => {
            let uri = format!("/hs-admin/post?{query}");
            let canonical = format!("{SITE}{uri}");
            let browser: &Browser = &BROWSER;
            let args = AttachmentArgs::parse(query, None)?;
            attachment_edit(&site, &args, &request(&canonical, &uri, browser))?
        }
    };
    let die = |status: u16, message: &str| {
        let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
        format!("{status}\t{}", String::from_utf8(page).unwrap())
    };
    Some(match answer {
        Answer::Page(page) => page,
        Answer::Redirect(to) => format!("302\t{to}"),
        Answer::Denied(message) => die(403, message),
        Answer::Missing(message) => die(404, message),
        Answer::Conflict(message) => die(409, message),
        Answer::Died(message) => die(500, message),
    })
}

/// Every refusal of `reference/post-edit/golden.sh`.
#[test]
fn refusals_are_php_s() {
    let golden = include_str!("../../tests/golden/post-edit.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let php = String::from_utf8(unhex(f[3])).unwrap();
        // A redirect leads to hs-admin's screen.
        let php = match php.starts_with("302") {
            true => hs_admin_page(&php),
            false => php,
        };
        let ours = answer(f[0], f[2], f[1]).unwrap_or_default();
        if ours != php {
            let difference = first_difference(&php, &ours);
            failures.push(format!("{} {} {}: {difference}", f[0], f[2], f[1]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(golden.lines().count(), 26);
}

/// The editor's page, the other registered types and arguments not ported
/// are not answered.
#[test]
fn the_editor_is_not_ported() {
    for (variant, role, screen) in [
        ("default", "administrator", "post-new.php"),
        ("default", "editor", "post-new.php?post_type=page"),
        (
            "default",
            "administrator",
            "post-new.php?post_type=wp_block",
        ),
        ("default", "subscriber", "post-new.php?post_type=revision"),
        (
            "default",
            "administrator",
            "post-new.php?post_type=page&x=1",
        ),
        ("default", "administrator", "post-new.php?post_type[]=page"),
        ("default", "author", "post.php?post=1&action=edit"),
        ("default", "editor", "post.php?post=2&action=edit"),
        ("a8lock", "administrator", "post.php?post=827&action=edit"),
    ] {
        assert_eq!(answer(variant, role, screen), None, "{role} {screen}");
    }
}

/// A missing theme's classic menu turns the same users away.
#[test]
fn a_missing_theme_refuses_alike() {
    for (role, screen) in [
        ("subscriber", "post-new.php"),
        ("contributor", "post-new.php?post_type=page"),
    ] {
        let refused = answer("default", role, screen);
        assert!(refused.as_deref().unwrap().starts_with("403\t"));
        assert_eq!(answer("g9gone", role, screen), refused, "{role} {screen}");
    }
}
