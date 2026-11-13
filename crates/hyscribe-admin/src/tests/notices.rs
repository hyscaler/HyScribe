// SPDX-License-Identifier: AGPL-3.0-or-later

//! The screens as PHP sends users back to them after an action, with the
//! notices of `reference/notices/golden.sh`.

use hyscribe_phpser::{addslashes, parse_str, remove_query_arg};
use hyscribe_render::date::{Zone, format_unix};
use hyscribe_testkit::{hs_admin_page, settled_page, unhex};

use crate::admin_bar::Browser;
use crate::comments::{CommentArgs, MODERATED, comment_order, edit_comments};
use crate::page::{REMOVABLE, Request};
use crate::posts::{Answer, BULK, ListArgs, PostType};
use crate::profile::{EditArgs, profile, user_edit};
use crate::terms::{Taxonomy, TermArgs};
use crate::tests::{Role, SITE, first_difference};
use crate::users::{UserArgs, user_order, users};

const BROWSER: Browser = Browser {
    user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
    mobile_hint: None,
};

/// The request's arguments, each a string.
fn arguments(query: &str) -> Vec<(String, String)> {
    let args = parse_str(query.as_bytes());
    args.as_array()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
            (text(key.to_php_string()), text(value.to_php_string()))
        })
        .collect()
}

/// The posts list's arguments of `query`.
fn list_args(query: &str) -> ListArgs {
    let mut args = ListArgs::default();
    for (key, value) in arguments(query) {
        match key.as_str() {
            "post_type" => args.post_type = PostType::named(value.as_bytes()).unwrap(),
            "post_status" => args.status = value,
            "paged" => args.paged = Some(value),
            "ids" => args.ids = Some(value),
            "skipped" => {}
            name => {
                let at = BULK.iter().position(|bulk| *bulk == name).unwrap();
                args.counts[at] = Some(value);
            }
        }
    }
    args
}

/// Our answer to the request of a golden line, under `canonical` and
/// `path`, the request slashed; `None` for one not ported.
fn ours(file: &str, query: &str, role: &'static str, request: &Request) -> Option<Answer> {
    let site = Role(role);
    Some(match file {
        "edit.php" => {
            return crate::posts::edit(
                &site,
                &list_args(query),
                request.canonical,
                request.origin,
                request.uri,
                request.browser,
            );
        }
        "edit-tags.php" => {
            let mut args = TermArgs {
                taxonomy: Taxonomy::named(&arguments(query)[0].1.clone().into_bytes()).unwrap(),
                paged: None,
                search: None,
                orderby: None,
                order: None,
                message: None,
                error: false,
            };
            for (key, value) in arguments(query) {
                match key.as_str() {
                    "taxonomy" => {}
                    "paged" => args.paged = Some(value),
                    "s" => args.search = Some(value),
                    "message" => args.message = Some(value),
                    _ => args.error = true,
                }
            }
            crate::terms::edit_tags(
                &site,
                &args,
                request.canonical,
                request.origin,
                request.uri,
                request.browser,
            )
        }
        "edit-comments.php" => {
            let mut args = CommentArgs::default();
            for (key, value) in arguments(query) {
                match key.as_str() {
                    "ids" => args.ids = Some(value),
                    "error" => args.error = Some(value),
                    "comment_status" => args.status = Some(value),
                    "p" => args.post = Some(value),
                    "paged" => args.paged = Some(value),
                    "unapproved" => {}
                    name => {
                        let at = MODERATED.iter().position(|m| *m == name).unwrap();
                        args.moderated[at] = Some(value);
                    }
                }
            }
            args.clauses = comment_order("", "").unwrap();
            let canonical = request.canonical;
            let (origin, uri, browser) = (request.origin, request.uri, request.browser);
            return edit_comments(&site, &args, None, canonical, origin, uri, browser);
        }
        "users.php" => {
            let mut args = UserArgs::default();
            for (key, value) in arguments(query) {
                let value = Some(value);
                match key.as_str() {
                    "update" => args.update = value,
                    "delete_count" => args.delete_count = value,
                    "reset_count" => args.reset_count = value,
                    "id" => args.id = value,
                    "role" => args.role = value,
                    "s" => args.search = value,
                    _ => args.paged = value,
                }
            }
            let roles = args.role.is_some();
            args.clauses = user_order(None, None, roles).unwrap();
            let (canonical, origin) = (request.canonical, request.origin);
            users(
                &site,
                &args,
                canonical,
                origin,
                request.uri,
                request.browser,
            )
        }
        "profile.php" | "user-edit.php" => {
            let mut args = EditArgs::default();
            for (key, value) in arguments(query) {
                let value = Some(value);
                match key.as_str() {
                    "user_id" => args.user_id = value,
                    "wp_http_referer" => args.referer = value,
                    "updated" => args.updated = true,
                    _ => args.error = value,
                }
            }
            if file == "profile.php" {
                profile(&site, &args, request)
            } else {
                user_edit(&site, &args, request)
            }
        }
        "user-new.php" => {
            let update = arguments(query)
                .into_iter()
                .find(|(key, _)| key == "update");
            let update = update.map(|(_, value)| value);
            crate::user_new::user_new(&site, update.as_deref(), request)
        }
        "options-general.php" => crate::settings::general(&site, request).unwrap(),
        "options-writing.php" => crate::settings::writing(&site, request),
        "options-reading.php" => crate::settings::reading(&site, request).unwrap(),
        "options-discussion.php" => crate::settings_more::discussion(&site, request),
        "options-media.php" => crate::settings_more::media(&site, request),
        "options-permalink.php" => crate::settings_more::permalink(&site, request),
        "options-privacy.php" => crate::settings_more::privacy(&site, request),
        _ => panic!("{file}"),
    })
}

/// Every page of `reference/notices/golden.sh`.
#[test]
fn notices_are_php_s() {
    let golden = include_str!("../../tests/golden/notices.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (file, query) = f[1].split_once('?').unwrap_or((f[1], ""));
        let role: &'static str = Box::leak(format!("{}:{}", f[0], f[2]).into_boxed_str());
        let role = role.strip_prefix("default:").unwrap_or(role);
        // The arguments as hs-admin is asked them, the request slashed.
        let query = hs_admin_page(query);
        let path = hs_admin_page(&format!("/wp-admin/{}", f[1]));
        let path = String::from_utf8(addslashes(path.as_bytes())).unwrap();
        let canonical = remove_query_arg(&REMOVABLE, format!("{SITE}{path}").as_bytes());
        let request = Request {
            canonical: &String::from_utf8(canonical).unwrap(),
            origin: SITE,
            uri: &path,
            browser: &BROWSER,
        };
        let recorded = String::from_utf8(unhex(f[3])).unwrap();
        let answer = ours(file, &query, role, &request);
        // PHP fails part way through the page, which is not ported.
        if recorded.contains("There has been a critical error") {
            assert!(answer.is_none(), "{}", f[1]);
            continue;
        }
        let Some(Answer::Page(ours)) = answer else {
            panic!("{}: no page", f[1]);
        };
        // The dates the settings print of now, at the test site's clock.
        let now = 1_794_500_000;
        let php = hs_admin_page(&recorded)
            .replace("%ymd%", &format_unix("Y/m/d", now, Zone::Utc))
            .replace("%ym%", &format_unix("Y/m", now, Zone::Utc));
        let (php, ours) = (settled_page(&php), settled_page(&ours));
        if ours != php {
            failures.push(format!(
                "{} {}: {}",
                f[1],
                f[2],
                first_difference(&php, &ours)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An Edit link to a restored post of a type not ported has no page.
#[test]
fn edit_links_of_other_types_are_not_ported() {
    let query = "untrashed=1&ids=9";
    let path = format!("/hs-admin/edit?{query}");
    let request = Request {
        canonical: &format!("{SITE}/hs-admin/edit"),
        origin: SITE,
        uri: &path,
        browser: &BROWSER,
    };
    assert!(ours("edit.php", query, "n6media:administrator", &request).is_none());
}

/// Edit User links back to Users unless sent from Add User, in PHP's form
/// or hs-admin's, and puts a relative referer in the request's folder, as
/// `wp_validate_redirect()` does.
#[test]
fn edit_user_links_back_to_users() {
    let link = |referer: &str| {
        let query = format!("user_id=601&updated=1&wp_http_referer={referer}");
        let path = format!("/hs-admin/user-edit?{query}");
        let request = Request {
            canonical: &format!("{SITE}/hs-admin/user-edit?user_id=601"),
            origin: SITE,
            uri: &path,
            browser: &BROWSER,
        };
        let Some(Answer::Page(page)) = ours("user-edit.php", &query, "u3:administrator", &request)
        else {
            panic!("{referer}: no page");
        };
        let (_, rest) = page.split_once("User updated.</strong></p>").unwrap();
        let end = rest.find("</div>").unwrap();
        rest.get(..end).unwrap().to_string()
    };
    for added in [
        "%2Fhs-admin%2Fuser-new",
        "%2Fhs-admin%2Fuser-new%3Fx",
        "%2Fhs-admin%2Fuser-new%23x",
    ] {
        assert_eq!(link(added), "", "{added}");
    }
    let others = "<p><a href=\"/hs-admin/user-newer\">&larr; Go to Users</a></p>";
    assert_eq!(link("%2Fhs-admin%2Fuser-newer"), others);
    let relative = "<p><a href=\"/hs-admin/users.php?role=editor\">&larr; Go to Users</a></p>";
    assert_eq!(link("users.php%3Frole%3Deditor"), relative);
}
