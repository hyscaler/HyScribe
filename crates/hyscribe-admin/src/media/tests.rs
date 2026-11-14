// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::remove_query_arg;
use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::posts::Answer;
use crate::tests::{Role, SITE, first_difference};

/// `wp_removable_query_args()` that the goldens' queries use.
const REMOVABLE: [&[u8]; 6] = [
    b"deleted",
    b"error",
    b"ids",
    b"message",
    b"trashed",
    b"untrashed",
];

/// `page` from its screen meta to the end of its content, the part that is
/// the screen's own.
pub(super) fn own(page: &str) -> &str {
    let start = page.find("<div id=\"screen-meta\"").unwrap_or(0);
    let end = page.find("<!-- wpbody-content -->").unwrap_or(page.len());
    page.get(start..end).unwrap_or_default()
}

/// `page` without its rows, for an order whose ties the database breaks.
fn rowless(page: &str) -> String {
    let start = page.find("<tbody id=\"the-list\"").unwrap_or(0);
    let end = page.find("\t</tbody>").unwrap_or(start);
    let (head, tail) = (page.get(..start), page.get(end..));
    format!("{}{}", head.unwrap_or_default(), tail.unwrap_or_default())
}

/// `page` with each nonce placeholder as `%nonce%`, as the hashed goldens
/// hold them.
pub(super) fn nonces(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(at) = rest.find("%nonce:") {
        out.push_str(rest.get(..at).unwrap_or_default());
        out.push_str("%nonce%");
        let tail = rest.get(at + 7..).unwrap_or_default();
        rest = tail
            .get(tail.find('%').map_or(0, |end| end + 1)..)
            .unwrap_or_default();
    }
    out.push_str(rest);
    out
}

pub(super) fn sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What a screen answers for a golden row: the page, a redirect as
/// `302\t` and its location, or `403` for a refusal.
fn answer(role: &'static str, screen: &str, variant: &str) -> String {
    let (file, query) = screen.split_once('?').unwrap_or((screen, ""));
    let file = file.trim_end_matches(".php");
    let uri = if query.is_empty() {
        format!("/hs-admin/{file}")
    } else {
        format!("/hs-admin/{file}?{query}")
    };
    let canonical = String::from_utf8(remove_query_arg(
        &REMOVABLE,
        format!("{SITE}{uri}").as_bytes(),
    ))
    .unwrap();
    let browser = Browser {
        user_agent: agent(variant),
        mobile_hint: None,
    };
    let request = crate::tests::request(&canonical, &uri, &browser);
    let site = Role(role);
    let answer = if file == "upload" {
        upload(&site, &LibraryArgs::parse(query).unwrap(), &request).unwrap()
    } else {
        let settings = (variant == "m4uploader").then(|| "uploader=1".to_string());
        media_new(&site, &NewArgs::parse(query, settings).unwrap(), &request)
    };
    match answer {
        Answer::Page(page) => page,
        Answer::Redirect(to) => format!("302\t{to}"),
        _ => "403".into(),
    }
}

/// The browser of a variant of `reference/media/golden.sh`.
fn agent(variant: &str) -> &'static [u8] {
    match variant {
        "m4ios5" => b"Mozilla/5.0 (iPhone; CPU iPhone OS 5_1 like Mac OS X) Mobile",
        "m4ios7" => b"Mozilla/5.0 (iPhone; CPU iPhone OS 7_0 like Mac OS X) Mobile",
        _ => b"curl",
    }
}

/// Compares the screens with a golden file's rows: whole pages, or over
/// the Theme Test Data, which the test site's frame does not hold, the
/// hashes of the screens' own parts. Returns the failures and the pages
/// compared.
fn compare(golden: &str, ttd: bool) -> (Vec<String>, usize) {
    let (mut failures, mut pages) = (Vec::new(), 0);
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let variant = match (ttd, f[0]) {
            (false, "default") => String::new(),
            (false, variant) => variant.to_string(),
            (true, "default") => "m4ttd".into(),
            (true, variant) => variant.replacen("m4", "m4ttd", 1),
        };
        let role: &'static str = if variant.is_empty() {
            Box::leak(f[2].to_string().into_boxed_str())
        } else {
            Box::leak(format!("{variant}:{}", f[2]).into_boxed_str())
        };
        let ours = answer(role, f[1], f[0]);
        let tied = f[1].contains("orderby=author") || f[1].contains("orderby=parent");
        // The fixture's pages are kept as hashes of their own parts.
        if let [_, _, _, own_hash, _] = f[..] {
            let ours = nonces(&ours);
            let ours = if tied { rowless(&ours) } else { ours };
            pages += 1;
            if sha256(own(&ours)) != own_hash {
                failures.push(format!("{role} {}: not PHP's", f[1]));
            }
            continue;
        }
        let recorded = String::from_utf8(unhex(f[3])).unwrap();
        if recorded.starts_with("403\t") {
            assert_eq!(ours, "403", "{role} {}", f[1]);
            continue;
        }
        let php = hs_admin_page(&recorded);
        pages += 1;
        if ours != php {
            failures.push(format!(
                "{role} {}: {}",
                f[1],
                first_difference(&php, &ours)
            ));
        }
    }
    (failures, pages)
}

/// Every page of `reference/media/golden.sh` over the default install.
#[test]
fn pages_are_php_s() {
    let (failures, pages) = compare(include_str!("../../tests/golden/media.tsv"), false);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(pages, 37);
}

/// Every page of `reference/media/golden.sh` over the Theme Test Data.
#[test]
fn theme_test_data_pages_are_php_s() {
    let (failures, pages) = compare(include_str!("../../tests/golden/media-ttd.tsv"), true);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(pages, 63);
}
/// Uploads need iOS 6 or later on an Apple device.
#[test]
fn uploads_follow_the_device() {
    let can = |agent: &'static [u8]| {
        can_upload(&Browser {
            user_agent: agent,
            mobile_hint: None,
        })
    };
    assert!(can(b"iPhone; CPU iPhone OS 6_0 like Mac OS X Mobile"));
    assert!(can(b"iPod; CPU OS 12 like Mac OS X Mobile"));
    assert!(!can(b"iPhone; CPU iPhone OS 5_1 like Mac OS X Mobile"));
    assert!(!can(b"iPhone; Mobile"));
    assert!(!can(b"iPhone; CPU OS 9_0 Mobile"));
    assert!(can(b"Macintosh; iPhone OS 5_1 like Mac OS X"));
}
