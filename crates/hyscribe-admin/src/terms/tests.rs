// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use std::collections::HashMap;

use hyscribe_model::TermQuery;
use hyscribe_phpser::{addslashes, remove_query_arg, urldecode};

use super::*;
use crate::tests::{Role, SITE, first_difference};

/// A query value as PHP decodes it.
fn decoded(value: &str) -> String {
    String::from_utf8(urldecode(value.as_bytes())).unwrap()
}

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
            search: arg("s").map(decoded),
            orderby: arg("orderby").map(decoded),
            order: arg("order").map(decoded),
            message: None,
            error: false,
        };
        // The request's path and query, slashed as `wp_magic_quotes()` does.
        let path = format!("/hs-admin/edit-tags?{query}");
        let path = String::from_utf8(addslashes(path.as_bytes())).unwrap();
        let role: &'static str = Box::leak(format!("{variant}:{}", f[1]).into_boxed_str());
        let ours = edit_tags(
            &Role(role),
            &args,
            &crate::tests::request(
                &String::from_utf8(remove_query_arg(
                    &[b"message"],
                    format!("{SITE}{path}").as_bytes(),
                ))
                .unwrap(),
                &path,
                &crate::tests::BROWSER,
            ),
        );
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        let (php, ours) = match ours {
            Answer::Page(page) => (hs_admin_page(&recorded), page),
            Answer::Redirect(to) => (hs_admin_page(&recorded), format!("302\t{to}")),
            Answer::Denied(message)
            | Answer::Died(message)
            | Answer::Missing(message)
            | Answer::Conflict(message) => {
                let page = crate::die::page(true, "HyScribe &rsaquo; Error", message);
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
        search: None,
        orderby: None,
        order: None,
        message: None,
        error: false,
    };
    let browser = crate::admin_bar::Browser {
        user_agent: b"Mozilla/5.0 (iPhone) Mobile",
        mobile_hint: None,
    };
    let path = "/hs-admin/edit-tags?taxonomy=post_tag";
    let Answer::Page(page) = edit_tags(
        &Role("administrator"),
        &args,
        &crate::tests::request(&format!("{SITE}{path}"), path, &browser),
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
        search: None,
        orderby: None,
        order: None,
        message: None,
        error: false,
    };
    let path = "/hs-admin/edit-tags?taxonomy=category";
    let browser = crate::admin_bar::Browser::default();
    let role = Role("stale:administrator");
    let Answer::Page(page) = edit_tags(
        &role,
        &args,
        &crate::tests::request(&format!("{SITE}{path}"), path, &browser),
    ) else {
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

/// The terms list `args` asks for on `variant`'s site: its term queries and
/// whether it read every term.
fn asked(variant: &'static str, args: &TermArgs) -> (Vec<TermQuery>, bool) {
    use crate::tests::{TERM_QUERIES, TERM_READS};
    TERM_QUERIES.with_borrow_mut(Vec::clear);
    TERM_READS.set(0);
    let path = "/hs-admin/edit-tags";
    let browser = crate::admin_bar::Browser::default();
    edit_tags(
        &Role(variant),
        args,
        &crate::tests::request(&format!("{SITE}{path}"), path, &browser),
    );
    (TERM_QUERIES.with_borrow(Clone::clone), TERM_READS.get() > 0)
}

/// Sorted categories are read whole and sliced, as PHP does; tags are read
/// a page at a time, and all of them only when one listed has a parent.
#[test]
fn what_is_read_for_a_page() {
    let args = |taxonomy, paged: Option<&str>| TermArgs {
        taxonomy,
        paged: paged.map(String::from),
        search: None,
        orderby: Some("name".into()),
        order: None,
        message: None,
        error: false,
    };
    let (queries, read) = asked("paged:administrator", &args(CATEGORY, Some("2")));
    assert_eq!((queries[0].limit, queries[0].offset, read), (None, 0, true));
    let (queries, read) = asked("paged:administrator", &args(POST_TAG, None));
    assert_eq!(
        (queries[0].limit, queries[0].offset, read),
        (Some(3), 0, false)
    );
    let (queries, read) = asked("paged:administrator", &args(POST_TAG, Some("2")));
    assert_eq!(
        (queries[0].limit, queries[0].offset, read),
        (Some(3), 3, true)
    );
}

/// A search's page is a slice of what it found.
#[test]
fn a_search_pages_what_it_found() {
    let role = Role("terms:administrator");
    let columns = columns(false);
    let terms: Vec<Term> = (1..=7)
        .map(|id| Term {
            id,
            ..Term::default()
        })
        .collect();
    let listing = Listing {
        site: &role,
        taxonomy: CATEGORY,
        terms: &terms,
        by_id: HashMap::new(),
        cached: None,
        columns: &columns,
        hidden: &[],
        request_uri: "/hs-admin/edit-tags?taxonomy=category&s=x",
    };
    assert_eq!(listing.tree(3, 6, true).matches("<tr ").count(), 3);
}

/// The body of `case` in `reference/admin-saves/golden.sh`, under
/// hs-admin's names.
fn saved_answer(case: &str) -> String {
    let golden = include_str!("../../tests/golden/admin-saves.tsv");
    let row = golden
        .lines()
        .find(|row| row.starts_with(&format!("{case}\t")));
    let body = row.unwrap().split('\t').nth(3).unwrap();
    hs_admin_page(&String::from_utf8(unhex(body)).unwrap())
}

/// The Ajax answers of Quick Edit and the add form are PHP's: the rows of
/// the term saved or added, with its fields and the notice.
#[test]
fn ajax_rows_are_php_s() {
    let saved = Role("t2saved:administrator");
    let term = |taxonomy, id| {
        let terms = crate::posts::PostsSite::terms(&saved, taxonomy);
        terms.into_iter().find(|term| term.id == id).unwrap()
    };
    let unchanged = saved_row(&Role("administrator"), CATEGORY, &term("category", 1), "");
    assert_eq!(unchanged, saved_answer("ajax-inline-unchanged"));
    let renamed = saved_row(&saved, POST_TAG, &term("post_tag", 801), "");
    assert_eq!(renamed, saved_answer("ajax-inline-rename"));
    let added = |id, term_taxonomy_id| Added {
        term: term(if id == 803 { "category" } else { "post_tag" }, id),
        term_group: 0,
        term_taxonomy_id,
    };
    let tag = term_added(
        &saved,
        POST_TAG,
        &added(802, 802),
        "/hs-admin/edit-tags?taxonomy=post_tag",
    );
    assert_eq!(tag, saved_answer("ajax-add-tag"));
    let child = term_added(&saved, CATEGORY, &added(803, 803), "");
    assert_eq!(child, saved_answer("ajax-add-child"));
    for (case, code, message) in [
        (
            "ajax-add-exists",
            "term_exists",
            "A term with the name provided already exists with this parent.",
        ),
        (
            "ajax-add-empty",
            "empty_term_name",
            "A name is required for this term.",
        ),
    ] {
        assert_eq!(add_failed(code, message), saved_answer(case));
    }
}
