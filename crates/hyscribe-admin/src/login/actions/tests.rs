// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{parse_str, urldecode};
use hyscribe_testkit::unhex;

use super::*;
use crate::login::Session;
use crate::tests::{Oracle, first_difference};

/// What PHP's Set-Cookie headers did to the reset password cookie.
fn reset(cookies: &str) -> Reset {
    match cookies.split_once('=') {
        None => Reset::Keep,
        Some((_, rest)) if rest.starts_with("%20; expires") => Reset::Expire,
        Some((_, rest)) => {
            let value = rest.split(';').next().unwrap();
            Reset::Set(urldecode(value.as_bytes()))
        }
    }
}

/// A GET with `query` and the reset password cookie.
fn request<'a>(query: &'a Value, cookie: Option<&'a [u8]>) -> Request<'a> {
    Request {
        query,
        form: None,
        test_cookie: false,
        logged_in_cookie: false,
        session: Session::None,
        reset_cookie: cookie,
        read_only: false,
    }
}

/// Every case of `reference/login-actions/golden.sh` but the post password
/// form's, which is not ported.
#[test]
fn the_other_actions_are_php_s() {
    let golden = include_str!("../../../tests/golden/login-actions.tsv");
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let query = parse_str(f[0].as_bytes());
        let cookie = f[1]
            .strip_prefix("rp:")
            .map(|value| urldecode(value.as_bytes()));
        let request = request(&query, cookie.as_deref());
        let oracle = Oracle {
            register: f[1] == "register",
            ..Oracle::default()
        };
        let uri = addslashes(format!("/wp-login.php?{}", f[0]).as_bytes());
        let ours = action(&oracle, &request, &request.action(), &uri);
        let php = unhex(f[5]);
        match (f[2], ours) {
            ("200", Action::Page(ours)) => {
                let php = String::from_utf8(php).unwrap();
                let ours = String::from_utf8(ours).unwrap();
                assert!(ours == php, "{line:.80}{}", first_difference(&php, &ours));
            }
            ("500", Action::Died(ours)) => assert_eq!(ours, php, "{line:.80}"),
            ("302", Action::Redirect(to, cookie)) => {
                let php = (f[3].as_bytes().to_vec(), reset(f[4]));
                assert_eq!((to, cookie), php, "{line:.80}");
            }
            (_, ours) => {
                assert_eq!(f[0], "action=postpass", "{line:.80}");
                assert_eq!(ours, Action::NotPorted);
            }
        }
    }
}

/// What would need `wp_verify_fast_hash()` or a write is not answered: a
/// stored reset key, a personal data request, and arrays.
#[test]
fn hashes_and_writes_are_not_ported() {
    let invalid = || {
        let to = Oracle::default().login_url("?action=lostpassword&error=invalidkey");
        Action::Redirect(to.into_bytes(), Reset::Expire)
    };
    let empty = Value::Array(Vec::new());
    for (stored, cookie, ours) in [
        (
            &b"1700000000:$generic$x"[..],
            &b"admin:abc"[..],
            Action::NotPorted,
        ),
        (b"$generic$x", b"admin:abc", Action::NotPorted),
        (b"1700000000:", b"admin:abc", invalid()),
        (b"1700000000:0", b"admin:abc", invalid()),
        (b"$generic$x", b"0:abc", invalid()),
        (b"$generic$x", b"admin:-", invalid()),
    ] {
        let oracle = Oracle {
            activation_key: stored,
            ..Oracle::default()
        };
        let ours_now = action(&oracle, &request(&empty, Some(cookie)), "rp", b"/");
        assert_eq!(ours_now, ours, "{cookie:?}");
    }
    let oracle = Oracle {
        user_request: Some(7),
        ..Oracle::default()
    };
    for (query, name) in [
        ("key%5B%5D=x&login=admin", "rp"),
        ("request_id=-7&confirm_key=x", "confirmaction"),
        ("", "postpass"),
    ] {
        let query = parse_str(query.as_bytes());
        assert_eq!(
            action(&oracle, &request(&query, None), name, b"/"),
            Action::NotPorted
        );
    }
}

/// `redirect_to` of "0" is `empty()`, so the form carries none.
#[test]
fn a_zero_redirect_is_none() {
    let oracle = Oracle::default();
    let page = |query: &str| {
        let query = parse_str(query.as_bytes());
        action(&oracle, &request(&query, None), "lostpassword", b"/")
    };
    assert_eq!(
        page("action=lostpassword&redirect_to=0"),
        page("action=lostpassword")
    );
}
