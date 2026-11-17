// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::remove_query_arg;
use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::media::tests::{nonces, own, sha256};
use crate::page::REMOVABLE;
use crate::tests::{Role, SITE, first_difference};

/// The browser of a variant of `reference/attachment-edit/golden.sh`.
fn agent(variant: &str) -> &'static [u8] {
    match variant.replacen("a8ttd", "a8", 1).as_str() {
        "a8lockchrome" | "a8locknorich" | "j9lockchrome" => {
            b"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
        }
        "a8lockfirefox" => b"Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0",
        "a8locksafari" => {
            b"Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15"
        }
        "a8lockiphone" | "j9lockiphone" => {
            b"Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari/604.1"
        }
        "a8lockoldiphone" => {
            b"Mozilla/5.0 (iPhone; U; CPU iPhone OS 4_0 like Mac OS X) AppleWebKit/532.9 (KHTML, like Gecko) Mobile/8A293 Safari/6531.22.7"
        }
        "a8lockedge" => b"Mozilla/5.0 (Windows NT 10.0) Edg/140.0",
        "a8lockopera" => b"Opera/9.80 (X11; Linux x86_64) Presto/2.12.388 Version/12.16",
        "a8lockoperamini" => b"Opera/9.80 (J2ME/MIDP; Opera Mini/9.80) Presto/2.5.25 Version/10.54",
        "a8lockie11" => b"Mozilla/5.0 (Windows NT 10.0; Trident/7.0; rv:11.0)",
        "a8lockie10" => b"Mozilla/5.0 (compatible; MSIE 10.0; Windows NT 6.1; Trident/6.0)",
        "a8lockmacie" => b"Mozilla/4.0 (compatible; MSIE 5.23; Mac_PowerPC) Gecko",
        "a8lockie" => b"Mozilla/4.0 (compatible; MSIE 5.0; Gecko)",
        "a8locklynx" => b"Lynx/2.8.9rel.1 libwww-FM/2.14 Gecko",
        _ => b"curl",
    }
}

/// The referer `reference/attachment-edit/golden.sh` sends in a variant.
fn sent_referer(variant: &str) -> Option<String> {
    match variant {
        "a8lockref" => Some(format!("{SITE}/hs-admin/upload?mode=list")),
        "a8lockrefpost" => Some(format!("{SITE}/hs-admin/post?post=1&action=edit")),
        _ => None,
    }
}

/// The screen's answer in `variant` to `role` sent from `referer`, as a
/// golden keeps it, `wp_die()`'s with its status; `None` if not ported.
fn answer(
    variant: &'static str,
    role: &str,
    screen: &str,
    referer: Option<String>,
) -> Option<String> {
    let query = screen.split_once('?').map_or("", |(_, query)| query);
    let uri = format!("/hs-admin/post?{query}");
    let canonical = remove_query_arg(&REMOVABLE, format!("{SITE}{uri}").as_bytes());
    let canonical = String::from_utf8(canonical).unwrap();
    let browser = Browser {
        user_agent: agent(variant),
        mobile_hint: None,
    };
    let request = Request {
        canonical: &canonical,
        origin: SITE,
        uri: &uri,
        browser: &browser,
    };
    let args = AttachmentArgs::parse(query, referer).unwrap();
    let site = Role(Box::leak(format!("{variant}:{role}").into_boxed_str()));
    let die = |status: u16, message: &str| {
        let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
        format!("{status}\t{}", String::from_utf8(page).unwrap())
    };
    Some(match attachment_edit(&site, &args, &request)? {
        Answer::Page(page) | Answer::Redirect(page) => page,
        Answer::Missing(message) => die(404, message),
        Answer::Conflict(message) => die(409, message),
        Answer::Denied(message) | Answer::Died(message) => die(403, message),
    })
}

/// The failures and pages compared of a golden file's rows: whole pages,
/// or over the Theme Test Data the hashes of their own parts.
fn compare(golden: &str, ttd: bool) -> (Vec<String>, usize) {
    let (mut failures, mut pages) = (Vec::new(), 0);
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let variant = match (ttd, f[0]) {
            (false, variant) => variant.to_string(),
            (true, "default") => "a8ttd".into(),
            (true, variant) => variant.replacen("a8", "a8ttd", 1),
        };
        let variant: &'static str = Box::leak(variant.into_boxed_str());
        let ours = answer(variant, f[2], f[1], sent_referer(variant)).unwrap_or_default();
        pages += 1;
        if let [_, _, _, own_hash, _] = f[..] {
            if sha256(own(&nonces(&ours))) != own_hash {
                failures.push(format!("{variant} {} {}: not PHP's", f[2], f[1]));
            }
            continue;
        }
        let php = String::from_utf8(unhex(f[3])).unwrap();
        let php = if php.starts_with("40") {
            php
        } else {
            hs_admin_page(&php)
        };
        if ours != php {
            failures.push(format!(
                "{variant} {} {}: {}",
                f[2],
                f[1],
                first_difference(&php, &ours)
            ));
        }
    }
    (failures, pages)
}

/// Every page of `reference/attachment-edit/golden.sh` over the default
/// install.
#[test]
fn pages_are_php_s() {
    let (failures, pages) = compare(
        include_str!("../../../tests/golden/attachment-edit.tsv"),
        false,
    );
    assert!(
        failures.is_empty(),
        "{}",
        failures[..failures.len().min(3)].join("\n")
    );
    assert_eq!(pages, 87);
}

/// Every page of `reference/attachment-edit/golden.sh` over the Theme Test
/// Data.
#[test]
fn theme_test_data_pages_are_php_s() {
    let golden = include_str!("../../../tests/golden/attachment-edit-ttd.tsv");
    let (failures, pages) = compare(golden, true);
    assert!(
        failures.is_empty(),
        "{}",
        failures[..failures.len().min(3)].join("\n")
    );
    assert_eq!(pages, 9);
}

/// Only `post` and `action=edit`, with the notice of a save, are ported.
#[test]
fn other_arguments_are_not_ported() {
    for query in [
        "post=1",
        "action=edit",
        "post=1&action=view",
        "post=1&action=edit&x=1",
        "post[]=1&action=edit",
    ] {
        assert!(AttachmentArgs::parse(query, None).is_none(), "{query}");
    }
}

/// Where PHP writes the user's own edit lock, or the screen does not port
/// the attachment's state, nothing is answered.
#[test]
fn unported_states_answer_nothing() {
    for (variant, post) in [
        ("a8lock", "0"),
        ("a8lock", "1"),
        ("a8lockold", "801"),
        ("a8lockself", "801"),
        ("a8lockzero", "801"),
        ("a8lockgone", "801"),
        ("a8lock", "901"),
        ("a8lock", "903"),
        ("a8lock", "904"),
        ("a8lock", "905"),
        ("j9pretty", "902"),
        ("j9pretty", "906"),
        ("j9pretty", "907"),
        ("j9prettyfront", "822"),
    ] {
        let screen = format!("post.php?post={post}&action=edit");
        assert!(
            answer(variant, "administrator", &screen, None).is_none(),
            "{variant} {post}"
        );
    }
}

/// A lock is fresh for 150 seconds, by `_edit_last` when it names no user;
/// this page or another site is no referer; a negative ID is missing.
#[test]
fn locks_referers_and_ids_are_php_s() {
    let screen = "post.php?post=801&action=edit";
    let page = answer("a8lock", "administrator", screen, None);
    for variant in ["a8locknew", "a8locklast"] {
        assert_eq!(
            answer(variant, "administrator", screen, None),
            page,
            "{variant}"
        );
    }
    let page = answer("a8lockcount", "administrator", screen, None);
    for referer in [
        "/hs-admin/post?post=801&action=edit",
        "http://127.0.0.1:8081/hs-admin/post?post=801&action=edit",
        "http://elsewhere.test/",
    ] {
        let sent = answer("a8lockcount", "administrator", screen, Some(referer.into()));
        assert_eq!(sent, page, "{referer}");
    }
    let missing = answer(
        "a8lock",
        "administrator",
        "post.php?post=9999&action=edit",
        None,
    );
    assert_eq!(
        answer(
            "a8lock",
            "administrator",
            "post.php?post=-5&action=edit",
            None
        ),
        missing
    );
}

/// `size_format()`'s units, from bytes to exabytes.
#[test]
fn sizes_are_php_s() {
    for (bytes, size) in [
        (0, "0 B"),
        (1023, "1,023 B"),
        (1536, "2 KB"),
        (5 << 30, "5 GB"),
        (3 << 40, "3 TB"),
        (1 << 50, "1 PB"),
        ((1 << 50) - 1, "1,024 TB"),
        // PHP compares as floats, which round this up to an exabyte.
        ((1 << 60) - 1, "1 EB"),
        (7 << 60, "7 EB"),
        (i64::MAX, "8 EB"),
    ] {
        assert_eq!(size_format(bytes), size);
    }
}

/// A file's extension is the word after its last dot.
#[test]
fn extensions_are_words() {
    assert_eq!(extension(b"a.tar.gz"), Some(&b"gz"[..]));
    assert_eq!(extension(b"a."), None);
    assert_eq!(extension(b"a.b-c"), None);
    assert_eq!(extension(b"notes"), None);
}

/// `human_readable_duration()`: hours, minutes and seconds in words, a sign
/// dropped; nothing for another form or a value that is not a string.
#[test]
fn durations_read_as_php_s() {
    let duration = |text: &str| human_readable_duration(&Value::String(text.into()));
    assert_eq!(duration(" -1:00:01 "), "1 hour, 0 minutes, 1 second");
    assert_eq!(duration("05:9"), "5 minutes, 9 seconds");
    for bad in ["", "59", "60:00", "1:2:3:4", "a:01", "1:61:00"] {
        assert_eq!(duration(bad), "", "{bad}");
    }
    assert_eq!(human_readable_duration(&Value::Int(61)), "");
}
