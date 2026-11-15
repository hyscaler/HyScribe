// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_auth::caps::{CapPost, User as Capabilities};
use hyscribe_phpser::parse_str;

use super::*;
use crate::tests::options;

/// User 1 with a role's capabilities over the default install and
/// `reference/action-checks/content.sql`, as
/// `reference/admin-menu/as-role.php` sets PHP up; a valid nonce is
/// `%nonce:<action>%`.
struct Oracle {
    role: &'static str,
    /// `_edit_lock` of every post.
    lock: bool,
}

fn role(role: &'static str) -> Oracle {
    Oracle { role, lock: false }
}

fn cap_post(id: u64, author: u64, status: &str, post_type: &str) -> CapPost {
    CapPost {
        id,
        author,
        status: status.into(),
        post_type: post_type.into(),
        parent: 0,
    }
}

impl CapabilityData for Oracle {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        options().get(name).cloned()
    }
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        let role = match id {
            1 => self.role,
            801 => "author",
            _ => return None,
        };
        // `role+cap` grants the role and a capability of its own.
        let caps: Vec<String> = role
            .split('+')
            .map(|cap| format!("s:{}:\"{cap}\";b:1;", cap.len()))
            .collect();
        (key == "wp_capabilities")
            .then(|| format!("a:{}:{{{}}}", caps.len(), caps.concat()).into_bytes())
    }
    fn user_exists(&self, id: u64) -> bool {
        matches!(id, 1 | 801)
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        match id {
            1 => Some(cap_post(1, 1, "publish", "post")),
            2 => Some(cap_post(2, 1, "publish", "page")),
            3 => Some(cap_post(3, 1, "draft", "page")),
            801 => Some(cap_post(801, 801, "publish", "post")),
            802 => Some(cap_post(802, 1, "trash", "post")),
            803 => Some(cap_post(803, 1, "publish", "unregistered")),
            _ => None,
        }
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        match (id, key) {
            (802, "_wp_trash_meta_status") => Some(b"publish".to_vec()),
            (_, "_edit_lock") if self.lock => Some(b"1793750400:801".to_vec()),
            _ => None,
        }
    }
    fn comment_post(&self, id: u64) -> Option<u64> {
        (id == 1).then_some(1)
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        match id {
            1 => Some("category".into()),
            801 => Some("post_tag".into()),
            _ => None,
        }
    }
    fn constant(&self, name: &str) -> bool {
        name == "DISALLOW_UNFILTERED_HTML"
    }
}

impl ActionSite for Oracle {
    fn current_user_can(&self, cap: &str, args: &[Value]) -> bool {
        Capabilities::load(self, "wp_", 1).has_cap(self, cap, args)
    }
    fn verify_nonce(&self, nonce: &[u8], action: &str) -> bool {
        nonce == format!("%nonce:{action}%").as_bytes()
    }
    fn validate_redirect(&self, location: &[u8]) -> Vec<u8> {
        let local = location.starts_with(b"http://127.0.0.1:8081/");
        if local { location.to_vec() } else { Vec::new() }
    }
    fn comment_approved(&self, id: u64) -> Option<Vec<u8>> {
        match id {
            1 => Some(b"1".to_vec()),
            2 => Some(b"spam".to_vec()),
            3 => Some(b"post-trashed".to_vec()),
            4 => Some(b"0".to_vec()),
            5 => Some(b"trash".to_vec()),
            _ => None,
        }
    }
    fn user_roles(&self) -> Option<Vec<u8>> {
        options().get("wp_user_roles").cloned()
    }
}

const HOME: &str = "http://127.0.0.1:8081";

/// `$_GET` or `$_POST` of `query`.
fn fields(query: &str) -> Vec<(Key, Value)> {
    match parse_str(query.as_bytes()) {
        Value::Array(fields) => fields,
        _ => Vec::new(),
    }
}

/// The checks of `target` (a screen and its query, as PHP's under
/// `/wp-admin/`) with `body`, `None` for one that is not an action.
fn check(
    site: &Oracle,
    target: &str,
    body: Option<&str>,
    referer: Option<&[u8]>,
) -> Option<Checked> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (get, post) = (fields(query), fields(body.unwrap_or_default()));
    let uri = format!("/wp-admin/{target}");
    let asked = Asked {
        get: &get,
        post: &post,
        referer,
        uri: uri.as_bytes(),
        home: HOME,
        user: 1,
    };
    let screen = path.strip_suffix(".php").unwrap();
    if screen == "admin-ajax" {
        return Some(admin_ajax(Some(site), &asked));
    }
    admin(site, screen, &asked, body.is_some())
}

/// What PHP sends for `refusal`, on the default install, which is public.
fn sent(refusal: &Refusal) -> Vec<u8> {
    match &refusal.body {
        Body::Page(title, message) => crate::die::page(true, title, message),
        Body::Text(text) => text.clone().into_bytes(),
        Body::Json(json) => json.as_bytes().to_vec(),
    }
}

/// Whether PHP's answer refuses: an error status, `wp_die()`'s page, or
/// an Ajax `-1` or `0`.
fn is_refusal(status: u16, body: &[u8]) -> bool {
    status >= 400
        || matches!(body, b"-1" | b"0")
        || String::from_utf8_lossy(body).contains("<body id=\"error-page\">")
}

/// The request of a golden row: the target and body, the nonce of the
/// variant placed in its field, and whether a referer is sent.
fn golden_request(case: &str, variant: &str) -> (String, Option<String>, bool) {
    let fields: Vec<&str> = case.split('\t').collect();
    let [_, method, target, body, action, field] = fields[..6] else {
        panic!("{case}");
    };
    let nonce = match variant {
        "none" => None,
        "wrong" => Some("0123456789".to_string()),
        "other" => Some("%other%".to_string()),
        _ => Some(format!("%nonce:{action}%")),
    };
    let mut data = (body != "-").then(|| body.to_string());
    let mut target = target.to_string();
    if let Some(nonce) = nonce {
        let field = format!("{field}={nonce}");
        match method {
            "POST" => data = Some(data.map_or(field.clone(), |d| format!("{d}&{field}"))),
            _ => target = format!("{target}&{field}"),
        }
    }
    (target, data, fields.len() > 6)
}

/// Every action of `reference/action-checks/golden.sh`, as each role with
/// each nonce, is refused as PHP refuses it, and answered 501 where PHP
/// goes on: to its first write, or to an answer that is not ported.
#[test]
fn refusals_match_php() {
    let cases = include_str!("../../../../reference/action-checks/cases.list");
    let golden = include_str!("../../tests/golden/action-checks.tsv");
    let referer = b"http://127.0.0.1:8081/wp-admin/edit.php?paged=2&updated=1";
    let (mut refused, mut accepted, mut failures) = (0, 0, Vec::new());
    for row in golden.lines() {
        let [who, name, variant, status, body] = row.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{row}");
        };
        let case = cases
            .lines()
            .find(|line| line.starts_with(&format!("{name}\t")));
        let (target, data, flagged) = golden_request(case.unwrap(), variant);
        let referer = flagged.then_some(&referer[..]);
        let what = format!("{who} {name} {variant}");
        let ours = check(&role(who), &target, data.as_deref(), referer).expect(&what);
        // PHP stopped before its first write: 200 and no body.
        let status: u16 = status.parse().unwrap_or(200);
        let php = hyscribe_testkit::unhex(body);
        match ours {
            Err(refusal) if refusal.status == status && sent(&refusal) == php => refused += 1,
            Ok(()) if !is_refusal(status, &php) => accepted += 1,
            ours => failures.push(format!(
                "{what}: {ours:?}\n  php: {status} {}",
                String::from_utf8_lossy(&php)
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(refused > 0 && accepted > 0);
}

/// The checks' outcome: none for a request that is not an action, the
/// status of a refusal, or 501 for a request that goes on.
fn outcome(site: &Oracle, target: &str, body: Option<&str>) -> Option<u16> {
    let checked = check(site, target, body, None);
    checked.map(|checked| checked.map_or_else(|refusal| refusal.status, |()| 501))
}

/// `_wpnonce` for `action`.
fn nonce(action: &str) -> String {
    format!("_wpnonce=%nonce:{action}%")
}

/// The action a request past its checks names for its handler.
#[test]
fn actions_are_named_as_php_names_them() {
    for (screen, query, expected) in [
        ("users", "changeit=Change&action=delete", "promote"),
        ("users", "action=resetpassword", "resetpassword"),
        ("users", "action=-1", ""),
        ("edit", "changeit=Change&action=trash", "trash"),
    ] {
        let get = fields(query);
        let asked = Asked {
            get: &get,
            post: &[],
            referer: None,
            uri: b"/wp-admin/",
            home: HOME,
            user: 1,
        };
        assert_eq!(
            action(screen, &asked),
            expected.as_bytes(),
            "{screen} {query}"
        );
    }
}

/// Requests the goldens leave out: those that are not actions, those whose
/// next step is not ported, and arguments PHP reads oddly.
#[test]
fn what_is_not_ported_goes_on() {
    let admin = role("administrator");
    let (bulk, users) = (nonce("bulk-posts"), nonce("bulk-users"));
    let update = format!("action=update&{}", nonce("update-options"));
    for (target, body, expected) in [
        ("edit.php".to_string(), None, None),
        ("edit.php?action=-1".into(), None, None),
        ("edit.php?action=".into(), None, None),
        (
            "edit.php?action=trash&filter_action=Filter".into(),
            None,
            None,
        ),
        (
            "edit.php?action=trash&post_type=nope".into(),
            None,
            Some(501),
        ),
        (
            format!("edit.php?action=trash&post_type=post&post[]=1&{bulk}"),
            None,
            Some(501),
        ),
        ("edit.php?action=trash&p=1".into(), None, Some(501)),
        (
            format!("edit.php?delete_all=1&post_status=trash&{bulk}"),
            None,
            Some(501),
        ),
        (
            format!("edit.php?action=trash&ids[]=1&{bulk}"),
            None,
            Some(501),
        ),
        (
            format!("edit.php?action=trash&post=1&{bulk}"),
            None,
            Some(501),
        ),
        (
            format!("edit.php?action=trash&media=0&{bulk}"),
            None,
            Some(501),
        ),
        (
            format!("edit.php?action=trash&post[]=&{bulk}"),
            None,
            Some(500),
        ),
        (
            format!("edit.php?action=edit&post[]=1&{bulk}"),
            None,
            Some(501),
        ),
        (
            format!("edit.php?action[]=trash&post[]=1&{bulk}"),
            None,
            Some(501),
        ),
        ("post.php?post=1".into(), None, None),
        ("post.php?action=edit&post=1".into(), None, None),
        (
            "post.php".into(),
            Some("action=editpost&post_ID=1".to_string()),
            Some(501),
        ),
        (
            format!("post.php?action=trash&post=-1&{}", nonce("trash-post_-1")),
            None,
            Some(410),
        ),
        (
            format!("post.php?action=trash&post=803&{}", nonce("trash-post_803")),
            None,
            Some(400),
        ),
        (
            "post.php".into(),
            Some("action=trash&post_ID=1&post_type[]=post".into()),
            Some(400),
        ),
        ("edit-tags.php?taxonomy=post_tag".into(), None, None),
        ("edit-tags.php?action=delete".into(), None, Some(501)),
        (
            "edit-tags.php?action=delete&taxonomy=nav_menu".into(),
            None,
            Some(501),
        ),
        (
            "edit-tags.php?action=delete&taxonomy=post_tag".into(),
            None,
            Some(501),
        ),
        (
            "edit-tags.php?action=edit&taxonomy=post_tag&tag_ID=801".into(),
            None,
            Some(501),
        ),
        ("edit-comments.php".into(), None, None),
        (
            format!("edit-comments.php?delete_all=1&{}", nonce("bulk-comments")),
            None,
            Some(501),
        ),
        ("users.php".into(), None, None),
        (
            format!("users.php?changeit=1&new_role=editor&{users}"),
            None,
            Some(501),
        ),
        (
            format!("users.php?changeit=1&new_role[]=editor&users[]=801&{users}"),
            None,
            Some(501),
        ),
        (
            format!("users.php?changeit=1&users[]=801&{users}"),
            None,
            Some(403),
        ),
        (
            format!("users.php?changeit=1&new_role=editor&users[]=1&{users}"),
            None,
            Some(501),
        ),
        (
            format!(
                "users.php?action=dodelete&users[]=801&{}",
                nonce("delete-users")
            ),
            None,
            Some(501),
        ),
        (
            format!("users.php?action=resetpassword&{users}"),
            None,
            Some(501),
        ),
        (
            format!("users.php?action=resetpassword&users[]=1&users[]=9&{users}"),
            None,
            Some(501),
        ),
        (
            format!("users.php?action=delete&users[]=801&{users}"),
            None,
            Some(501),
        ),
        (
            "users.php?action=custom&users[]=801".into(),
            None,
            Some(501),
        ),
        ("user-new.php".into(), None, None),
        (
            "user-new.php".into(),
            Some("action=adduser&_wpnonce_add-user=%nonce:add-user%".into()),
            Some(501),
        ),
        ("profile.php".into(), None, None),
        (
            "profile.php".into(),
            Some("action=update&user_id=9".into()),
            Some(500),
        ),
        (
            "user-edit.php".into(),
            Some("action=update".into()),
            Some(500),
        ),
        ("profile.php?newuseremail=x".into(), None, Some(501)),
        (
            format!(
                "profile.php?dismiss=1_new_email&{}",
                nonce("dismiss-1_new_email")
            ),
            None,
            Some(501),
        ),
        ("profile.php?dismiss=1_new_email".into(), None, Some(403)),
        (
            "user-edit.php?user_id=801&dismiss=1_new_email".into(),
            None,
            None,
        ),
        ("options.php".into(), None, Some(501)),
        ("options.php?adminhash=x".into(), None, Some(501)),
        (
            "options.php?dismiss=new_admin_email".into(),
            None,
            Some(403),
        ),
        ("options.php".into(), Some(update.clone()), Some(501)),
        (
            "options.php?option_page=options".into(),
            Some(update),
            Some(501),
        ),
        (
            "options.php".into(),
            Some(format!(
                "action=update&option_page=0&{}",
                nonce("0-options")
            )),
            Some(403),
        ),
        (
            "options.php".into(),
            Some(format!(
                "action=update&option_page=misc&{}",
                nonce("misc-options")
            )),
            Some(501),
        ),
        ("options-permalink.php".into(), None, None),
        (
            "options-permalink.php".into(),
            Some("category_base=x".into()),
            Some(403),
        ),
        ("themes.php".into(), None, None),
        (
            "themes.php?action=delete&stylesheet=twentytwentyfive".into(),
            None,
            Some(501),
        ),
        (
            format!("themes.php?action=activate&{}", nonce("switch-theme_")),
            None,
            Some(501),
        ),
        ("index.php?action=trash".into(), None, None),
        (
            "edit.php?action=trash&post_type=post&post[]=1".into(),
            None,
            Some(403),
        ),
        (
            "users.php?action=delete&users[]=801".into(),
            None,
            Some(403),
        ),
        (
            "options.php?adminhash=x".into(),
            Some("action=update".into()),
            Some(501),
        ),
        (
            "options.php?adminhash=0".into(),
            Some("action=update".into()),
            Some(403),
        ),
        (
            "options.php?option_page=misc".into(),
            Some(format!("action=update&{}", nonce("misc-options"))),
            Some(501),
        ),
    ] {
        assert_eq!(
            outcome(&admin, &target, body.as_deref()),
            expected,
            "{target} {body:?}"
        );
    }
}

/// The checks that read a lock, which is not ported, or the role.
#[test]
fn locks_and_roles() {
    let locked = Oracle {
        role: "administrator",
        lock: true,
    };
    let bulk = nonce("bulk-posts");
    let trash = format!("edit.php?action=trash&post[]=1&{bulk}");
    assert_eq!(outcome(&locked, &trash, None), Some(501));
    let untrash = format!("edit.php?action=untrash&post[]=1&{bulk}");
    assert_eq!(outcome(&locked, &untrash, None), Some(500));
    let single = format!("post.php?action=trash&post=1&{}", nonce("trash-post_1"));
    assert_eq!(outcome(&locked, &single, None), Some(501));
    // The trashed post would refuse, were it not locked.
    let trashed = format!("edit.php?action=trash&post[]=802&{bulk}");
    assert_eq!(outcome(&locked, &trashed, None), Some(501));
    let trashed = format!("post.php?action=trash&post=802&{}", nonce("trash-post_802"));
    assert_eq!(outcome(&locked, &trashed, None), Some(501));
    let untrash = format!("post.php?action=untrash&post=1&{}", nonce("untrash-post_1"));
    assert_eq!(outcome(&locked, &untrash, None), Some(500));
    let activate = "themes.php?action=activate";
    let options = role("editor+edit_theme_options");
    assert_eq!(outcome(&options, activate, None), Some(501));
    assert_eq!(outcome(&role("subscriber"), activate, None), Some(403));
}

/// `wp_get_referer()` prefers `_wp_http_referer`, and gives nothing for the
/// page itself or another site.
#[test]
fn referers() {
    let admin = role("administrator");
    let link = |target: &str, referer: Option<&[u8]>| {
        let Some(Err(Refusal {
            body: Body::Page(_, message),
            ..
        })) = check(&admin, target, None, referer)
        else {
            panic!("{target}");
        };
        message.contains("href")
    };
    let given = "edit.php?action=trash&_wp_http_referer=http://127.0.0.1:8081/wp-admin/x.php";
    assert!(link(given, None));
    let target = "edit.php?action=trash";
    assert!(!link(target, Some(b"/wp-admin/edit.php?action=trash")));
    assert!(!link(
        target,
        Some(b"http://127.0.0.1:8081/wp-admin/edit.php?action=trash")
    ));
    assert!(!link(target, Some(b"http://elsewhere.test/")));
    assert!(!link(target, Some(b"0")));
}

/// `admin-ajax.php` before the actions: the action given, and the user.
#[test]
fn ajax_shell() {
    let ajax = |site: Option<&dyn ActionSite>, query: &str, body: &str| {
        let (get, post) = (fields(query), fields(body));
        let asked = Asked {
            get: &get,
            post: &post,
            referer: None,
            uri: b"/wp-admin/admin-ajax.php",
            home: HOME,
            user: 1,
        };
        admin_ajax(site, &asked).map_or_else(|refusal| refusal.status, |()| 501)
    };
    assert_eq!(ajax(None, "", "action[]=x"), 400);
    assert_eq!(ajax(None, "", "action=heartbeat"), 501);
    assert_eq!(ajax(None, "", "action=generate-password"), 501);
    assert_eq!(ajax(None, "", "action=delete-tag"), 400);
    let admin = role("administrator");
    let site: Option<&dyn ActionSite> = Some(&admin);
    assert_eq!(ajax(site, "", "action=x&wp_customize=on"), 501);
    assert_eq!(ajax(site, "", "action=check_plugin_dependencies"), 501);
    assert_eq!(ajax(site, "action=rest-nonce", ""), 501);
    assert_eq!(ajax(site, "action=heartbeat", ""), 400);
    assert_eq!(ajax(site, "action=rest-nonce", "action=heartbeat"), 501);
}

/// The users list's refusals of a role that may list users but not change
/// them, which no core role is, and of a term that does not exist.
#[test]
fn refusals_of_other_roles() {
    let lister = role("editor+list_users");
    let users = nonce("bulk-users");
    for (target, expected) in [
        (
            format!("users.php?changeit=1&new_role=editor&users[]=801&{users}"),
            403,
        ),
        (
            format!("users.php?action=resetpassword&users[]=801&{users}"),
            500,
        ),
        (
            format!("users.php?action=resetpassword&users[]=1&{users}"),
            501,
        ),
        (
            format!(
                "users.php?action=dodelete&users[]=801&delete_option=delete&{}",
                nonce("delete-users")
            ),
            403,
        ),
    ] {
        assert_eq!(outcome(&lister, &target, None), Some(expected), "{target}");
    }
    let deleting = format!(
        "users.php?action=dodelete&users[]=801&{}",
        nonce("delete-users")
    );
    assert_eq!(outcome(&lister, &deleting, None), Some(501));
    let resetting = format!("users.php?action=resetpassword&users[]=1&users[]=801&{users}");
    assert_eq!(outcome(&lister, &resetting, None), Some(500));
    let tag = "action=add-tag&taxonomy=&_wpnonce_add-tag=%nonce:add-tag%";
    assert_eq!(
        outcome(&role("contributor"), "admin-ajax.php", Some(tag)),
        Some(200)
    );
    let edited = format!(
        "action=editedtag&taxonomy=post_tag&tag_ID=9&{}",
        nonce("update-tag_9")
    );
    let admin = role("administrator");
    assert_eq!(outcome(&admin, "edit-tags.php", Some(&edited)), Some(403));
}

/// The Ajax actions' checks past their nonce, where the goldens do not go.
#[test]
fn ajax_actions() {
    let admin = role("administrator");
    for (body, expected) in [
        (
            "action=add-tag&taxonomy=nope&_wpnonce_add-tag=%nonce:add-tag%",
            501,
        ),
        ("action=add-tag&_wpnonce_add-tag=%nonce:add-tag%", 501),
        (
            "action=inline-save-tax&taxonomy=post_tag&_inline_edit=%nonce:taxinlineeditnonce%",
            200,
        ),
        (
            "action=inline-save&_inline_edit=%nonce:inlineeditnonce%",
            200,
        ),
        (
            "action=closed-postboxes&hidden[]=x&closedpostboxesnonce=%nonce:closedpostboxes%",
            501,
        ),
        (
            "action=hidden-columns&page[]=x&screenoptionnonce=%nonce:screen-options-nonce%",
            200,
        ),
        (
            "action=meta-box-order&_ajax_nonce=%nonce:meta-box-order%",
            501,
        ),
        ("action=delete-comment&id=9", 501),
        ("action=delete-comment&id=-1", 501),
        ("action=dim-comment&id=1&new=approved", 501),
        ("action=dim-comment&id=2&new=spam", 501),
        ("action=dim-comment&id=4&new=unapproved", 501),
        ("action=dim-comment&id=5&new=trash", 501),
        ("action=dim-comment&id=3&new=", 403),
        (
            "action=dim-comment&id=3&_wpnonce=%nonce:approve-comment_3%",
            501,
        ),
        (
            "action=save-user-color-scheme&nonce=%nonce:save-color-scheme%",
            200,
        ),
    ] {
        assert_eq!(
            outcome(&admin, "admin-ajax.php", Some(body)),
            Some(expected),
            "{body}"
        );
    }
    let dim = "action=dim-comment&id=2&_wpnonce=%nonce:approve-comment_2%";
    assert_eq!(
        outcome(&role("contributor"), "admin-ajax.php", Some(dim)),
        Some(200)
    );
}

/// `post.php`'s Quick Draft: `exit` for who may not write posts, the form
/// for a bad nonce or an empty form, else the referer of its post's type
/// and `edit_post()`'s capability; and the Screen Options form on any screen.
#[test]
fn quick_drafts_and_screen_options() {
    let draft = |id: &str, rest: &str| {
        format!(
            "action=post-quickdraft-save&post_ID={id}&post_title=T&_wpnonce=%nonce:add-post%{rest}"
        )
    };
    for (who, body, expected) in [
        ("subscriber", draft("1", ""), 200),
        ("administrator", draft("1", "&_wpnonce=bad"), 501),
        ("administrator", draft("99", ""), 403),
        ("administrator", draft("2", ""), 403),
        ("administrator", draft("1", "&post_type=post"), 501),
        ("author", draft("801", ""), 500),
        ("author", draft("801", "&post_title=+"), 501),
        ("author", draft("801", "&content[]=x"), 501),
        ("author", draft("801", "&post_title[]=x"), 501),
    ] {
        assert_eq!(
            outcome(&role(who), "post.php", Some(&body)),
            Some(expected),
            "{who} {body}"
        );
    }
    let admin = role("administrator");
    assert_eq!(
        outcome(
            &admin,
            "post.php?action=post-quickdraft-save&post_ID=1",
            None
        ),
        None
    );
    let options = "wp_screen_options[option]=edit_per_page&screenoptionnonce=";
    let valid = format!("{options}%nonce:screen-options-nonce%");
    for (target, body, expected) in [
        ("index.php", valid.as_str(), Some(501)),
        ("edit.php", options, Some(403)),
        ("index.php", "wp_screen_options=x", None),
    ] {
        assert_eq!(
            outcome(&admin, target, Some(body)),
            expected,
            "{target} {body}"
        );
    }
    let post = fields(&valid);
    for (screen, expected) in [("edit", "screen-options"), ("admin-ajax", "")] {
        let asked = Asked {
            get: &[],
            post: &post,
            referer: None,
            uri: b"/wp-admin/",
            home: HOME,
            user: 1,
        };
        assert_eq!(action(screen, &asked), expected.as_bytes(), "{screen}");
    }
}
