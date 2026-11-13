// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use std::collections::HashMap;

use super::*;
use crate::tests::{Role, SITE, first_difference};

/// Every page of `reference/terms-list/golden.sh`; a refusal is `403`.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../tests/golden/terms-list.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (variant, query) = f[0].split_once('?').unwrap();
        let variant = if variant == "content" {
            "terms"
        } else {
            variant
        };
        let arg = |name: &str| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        };
        let args = TermArgs {
            taxonomy: Taxonomy::named(arg("taxonomy").unwrap().as_bytes()).unwrap(),
            paged: arg("paged").map(String::from),
        };
        let path = format!("/hs-admin/edit-tags?{query}");
        let role: &'static str = Box::leak(format!("{variant}:{}", f[1]).into_boxed_str());
        let ours = edit_tags(
            &Role(role),
            &args,
            &format!("{SITE}{path}"),
            SITE,
            &path,
            &crate::admin_bar::Browser {
                user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
                mobile_hint: None,
            },
        );
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        let (php, ours) = match ours {
            Answer::Page(page) => (hs_admin_page(&recorded), page),
            Answer::Redirect(to) => (hs_admin_page(&recorded), format!("302\t{to}")),
            Answer::Denied(message) => {
                let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
                (
                    recorded,
                    format!("403\t{}", String::from_utf8(page).unwrap()),
                )
            }
        };
        if ours != php {
            failures.push(format!("{role}?{query}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A term's ancestors stop at one missing or repeated.
#[test]
fn ancestors_stop_at_a_missing_or_repeated_term() {
    let by_id = |terms: &[Term]| -> Vec<u64> {
        let map: HashMap<u64, &Term> = terms.iter().map(|term| (term.id, term)).collect();
        ancestors(&map, &terms[0])
    };
    let terms = content::terms("category");
    let zeta: Vec<Term> = terms
        .iter()
        .filter(|term| term.id == 406)
        .cloned()
        .collect();
    assert_eq!(by_id(&zeta), [499]);
    let looped = [
        Term {
            id: 1,
            parent: 2,
            ..Term::default()
        },
        Term {
            id: 2,
            parent: 1,
            ..Term::default()
        },
    ];
    assert_eq!(by_id(&looped), [2, 1]);
}

/// On a phone the name field is not focused.
#[test]
fn phones_get_no_focus_script() {
    let args = TermArgs {
        taxonomy: POST_TAG,
        paged: None,
    };
    let browser = crate::admin_bar::Browser {
        user_agent: b"Mozilla/5.0 (iPhone) Mobile",
        mobile_hint: None,
    };
    let path = "/hs-admin/edit-tags?taxonomy=post_tag";
    let Answer::Page(page) = edit_tags(
        &Role("administrator"),
        &args,
        &format!("{SITE}{path}"),
        SITE,
        path,
        &browser,
    ) else {
        panic!("no page");
    };
    assert!(!page.contains("focus()"));
}

/// A description the filters leave blank shows the dash.
#[test]
fn blank_descriptions_show_the_dash() {
    assert!(column_description(b"<script></script>").contains("No description"));
    assert_eq!(column_description(b"x"), "<p>x</p>\n");
}

/// A term the user may not edit, whose name sanitizes away, is shown
/// without its edit links or label.
#[test]
fn uneditable_terms_have_no_edit_links() {
    let role = Role("terms:administrator");
    let columns = columns(true);
    let term = Term {
        id: 999,
        name: b"   ".to_vec(),
        slug: b"x".to_vec(),
        ..Term::default()
    };
    let listing = Listing {
        site: &role,
        taxonomy: CATEGORY,
        terms: &[],
        by_id: HashMap::new(),
        cached: None,
        columns: &columns,
        hidden: &[],
        request_uri: "/hs-admin/edit-tags?taxonomy=category",
    };
    let row = listing.row(&term, 0);
    assert!(!row.contains("row-title") && !row.contains("editinline"));
    assert!(!row.contains("aria-label=\"\"") && row.contains("<strong></strong>"));
}

/// PHP opens only the parents its cached hierarchy names.
#[test]
fn a_stale_hierarchy_cache_hides_children() {
    let args = TermArgs {
        taxonomy: CATEGORY,
        paged: None,
    };
    let path = "/hs-admin/edit-tags?taxonomy=category";
    let browser = crate::admin_bar::Browser::default();
    let role = Role("stale:administrator");
    let Answer::Page(page) =
        edit_tags(&role, &args, &format!("{SITE}{path}"), SITE, path, &browser)
    else {
        panic!("no page");
    };
    assert!(page.contains("id=\"tag-401\"") && !page.contains("id=\"tag-402\""));
}

/// A name's raw ampersand is kept as `wp_kses_data()` writes it.
#[test]
fn names_go_through_kses() {
    assert_eq!(crate::posts::term_name(b"A & B"), "A &amp; B");
}

/// Each taxonomy names its own capability, which `map_meta_cap()` maps.
#[test]
fn taxonomies_name_their_capabilities() {
    assert_eq!(CATEGORY.manage_cap(), "manage_categories");
    assert_eq!(POST_TAG.manage_cap(), "manage_post_tags");
}

/// The cached hierarchy's parents are its integer keys; anything but an
/// array is no cache.
#[test]
fn cached_parents_are_the_array_s_keys() {
    let parents = cached_parents(b"a:2:{i:401;a:1:{i:0;i:402;}s:1:\"x\";a:0:{}}").unwrap();
    assert_eq!(parents.into_iter().collect::<Vec<_>>(), [401]);
    assert!(cached_parents(b"s:1:\"x\";").is_none());
}
