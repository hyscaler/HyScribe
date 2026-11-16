// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::Oracle;

const PASSWORD: &[u8] = b"reference-only-password";

fn names(filters: &Authenticate) -> Vec<(&'static str, i32)> {
    filters.0.iter().map(|f| (f.name, f.priority)).collect()
}

/// The codes of a failure, or the user who logged in.
fn outcome(result: Result<Account, Vec<Notice>>) -> Result<(u64, Vec<u8>), Vec<&'static str>> {
    result
        .map(|user| (user.id, user.login))
        .map_err(|errors| errors.iter().map(|e| e.code).collect())
}

/// `wp_signon()`'s chain: core's callbacks at their priorities, and a
/// site's added after those of its priority or lower.
#[test]
fn the_chain_runs_by_priority_then_as_added() {
    let oracle = Oracle::default();
    let mut filters = core(&oracle, &Session::None, false);
    let core_names = [
        ("wp_authenticate_username_password", 20),
        ("wp_authenticate_email_password", 20),
        ("wp_authenticate_cookie", 30),
        ("wp_authenticate_spam_check", 99),
    ];
    assert_eq!(names(&filters), core_names);
    for (name, priority) in [("late", 20), ("first", 10), ("between", 25), ("last", 99)] {
        filters.add(name, priority, Box::new(|user, _, _| user));
    }
    assert_eq!(
        names(&filters),
        [
            ("first", 10),
            core_names[0],
            core_names[1],
            ("late", 20),
            ("between", 25),
            core_names[2],
            core_names[3],
            ("last", 99),
        ]
    );
}

/// A callback that decides first is not overridden by core's.
#[test]
fn an_earlier_callback_s_user_stands() {
    let oracle = Oracle::default();
    let session = Session::Expired;
    let mut filters = core(&oracle, &session, false);
    let user = |_, _: &[u8], _: &[u8]| {
        Authenticated::User(Account {
            id: 9,
            login: b"nine".to_vec(),
        })
    };
    filters.add("two_factor", 10, Box::new(user));
    for (name, password) in [("admin", &b"wrong"[..]), ("", b"")] {
        let result = wp_authenticate(&oracle, &filters, name.as_bytes(), password);
        assert_eq!(outcome(result), Ok((9, b"nine".to_vec())), "{name}");
    }
    assert!(oracle.fired.borrow().is_empty());
}

/// A replaced callback runs in its place; a name not in the chain replaces
/// nothing, and no callback deciding fails the login.
#[test]
fn a_callback_is_replaced_in_place() {
    let oracle = Oracle::default();
    let mut filters = core(&oracle, &Session::None, false);
    let pass = || -> Callback { Box::new(|user, _, _| user) };
    assert!(!filters.replace("limit_login_attempts", pass()));
    assert!(filters.replace("wp_authenticate_username_password", pass()));
    assert_eq!(names(&filters)[0].0, "wp_authenticate_username_password");
    // Only the email callback is left to log in.
    let by_name = wp_authenticate(&oracle, &filters, b"admin", PASSWORD);
    assert_eq!(outcome(by_name), Err(vec!["authentication_failed"]));
    let email = b"admin@default-install.test";
    let by_email = wp_authenticate(&oracle, &filters, email, PASSWORD);
    assert_eq!(outcome(by_email), Ok((1, b"admin".to_vec())));
    let no_password = wp_authenticate(&oracle, &filters, email, b"");
    assert_eq!(outcome(no_password), Err(vec!["empty_password"]));
    // Its own blank fields' errors.
    let Err(errors) = wp_authenticate(&oracle, &filters, b"", b"") else {
        panic!("blank fields logged in");
    };
    let texts: Vec<&str> = errors.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "<strong>Error:</strong> The email field is empty.",
            "<strong>Error:</strong> The password field is empty."
        ]
    );
    assert_eq!(
        *oracle.fired.borrow(),
        ["wp_login_failed admin [\"authentication_failed\"]"]
    );
}

/// `wp_login_failed` fires with the sanitized name and the error, except
/// for a blank field.
#[test]
fn a_failure_fires_wp_login_failed() {
    let oracle = Oracle::default();
    let expired = Session::Expired;
    let cases: [(&Session, &[u8], &[u8]); 6] = [
        (&Session::None, b"admin", b"wrong"),
        (&Session::None, b" no<b>body ", b"x"),
        (&Session::None, b"nobody@x.test", b"x"),
        (&Session::None, b"admin", b" "),
        (&Session::None, b"", b"x"),
        (&expired, b"", b""),
    ];
    for (session, name, password) in cases {
        let filters = core(&oracle, session, false);
        assert!(wp_authenticate(&oracle, &filters, name, password).is_err());
    }
    assert_eq!(
        *oracle.fired.borrow(),
        [
            "wp_login_failed admin [\"incorrect_password\"]",
            "wp_login_failed nobody [\"invalid_username\"]",
            "wp_login_failed nobody@x.test [\"invalid_email\"]",
            "wp_login_failed  [\"expired_session\"]",
        ]
    );
}
