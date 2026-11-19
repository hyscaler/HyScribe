// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{parse_str, urldecode};
use hyscribe_testkit::unhex;

use super::*;
use crate::login::Session;
use crate::tests::{Oracle, REQUESTED, first_difference};

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
        let ours = action(&oracle, &request, &request.action(), (&uri, NOW));
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

/// A time after the goldens' keys were asked for and before the good ones
/// expire.
const NOW: i64 = 1_800_000_000;

/// The page with the reset form's suggested password masked, as the
/// golden has it.
fn masked(page: Vec<u8>) -> String {
    let page = String::from_utf8(page).unwrap();
    match page.split_once("data-pw=\"") {
        Some((head, rest)) => {
            let rest = rest.split_once('"').unwrap().1;
            format!("{head}data-pw=\"%pw%\"{rest}")
        }
        None => page,
    }
}

/// Every reset password case of `reference/lost-password/golden.sh`: the
/// form, its errors, the redirects of bad and expired keys, and the resets,
/// with the page after them.
#[test]
fn the_reset_password_flows_are_php_s() {
    let golden = include_str!("../../../tests/golden/lost-password.tsv");
    let oracle = Oracle {
        lost: true,
        ..Oracle::default()
    };
    let mut resets = Vec::new();
    let reset = |line: &&str| line.contains("\taction=rp") || line.contains("\taction=resetpass");
    for line in golden.lines().filter(reset) {
        let f: Vec<&str> = line.split('\t').collect();
        let (query, form) = (parse_str(f[1].as_bytes()), parse_str(f[3].as_bytes()));
        let cookie = urldecode(f[2].as_bytes());
        let request = Request {
            form: (f[0] == "POST").then_some(&form),
            ..request(&query, (!f[2].is_empty()).then_some(&cookie[..]))
        };
        let name = request.action();
        let php = String::from_utf8(unhex(f[7])).unwrap();
        match (f[4], action(&oracle, &request, &name, (b"/", NOW))) {
            ("200", Action::Page(ours)) => {
                let ours = masked(ours);
                assert!(ours == php, "{line:.80}{}", first_difference(&php, &ours));
            }
            ("200", Action::ResetPassword(id, password)) => {
                resets.push((id, String::from_utf8(password).unwrap()));
                let ours = String::from_utf8(password_reset(&oracle, &request, &name)).unwrap();
                assert!(ours == php, "{line:.80}{}", first_difference(&php, &ours));
            }
            ("302", Action::Redirect(to, Reset::Expire)) => {
                assert_eq!(to, f[5].as_bytes(), "{line:.80}");
            }
            (_, ours) => panic!("{line:.80}: {ours:?}"),
        }
    }
    assert_eq!(resets, [(701, "one".into()), (706, "It\\'s new".into())]);
}

/// The lost password form shows the error a posted form met, with the
/// login it posted.
#[test]
fn a_refused_lost_password_form_is_php_s() {
    let golden = include_str!("../../../tests/golden/lost-password.tsv");
    for (form, code, text) in [
        (
            "user_login=",
            "empty_username",
            "<strong>Error:</strong> Please enter a username or email address.",
        ),
        (
            "user_login=nobody%22%3Cb%3E",
            "invalidcombo",
            "<strong>Error:</strong> There is no account with that username or email address.",
        ),
    ] {
        let line = format!("POST\taction=lostpassword\t\t{form}\t200");
        let line = golden.lines().find(|l| l.starts_with(&line)).unwrap();
        let php = String::from_utf8(unhex(line.rsplit('\t').next().unwrap())).unwrap();
        let (query, form) = (
            parse_str(b"action=lostpassword"),
            parse_str(form.as_bytes()),
        );
        let request = Request {
            form: Some(&form),
            ..request(&query, None)
        };
        let page = lost_password(
            &Oracle::default(),
            &request,
            "lostpassword",
            Some((code, text)),
        );
        assert_eq!(String::from_utf8(page).unwrap(), php, "{line:.80}");
    }
}

/// Keys PHP finds invalid or expired, and what PHP fails on with a
/// `TypeError` (not answered): a stored time with no number, a posted key
/// or password that is an array or missing, a link's key that is an array,
/// and a personal data request, whose confirmation writes.
#[test]
fn what_php_refuses_is_php_s_or_not_ported() {
    let lost = |error: &str| {
        let to = Oracle::default().login_url(&format!("?action=lostpassword&error={error}"));
        Action::Redirect(to.into_bytes(), Reset::Expire)
    };
    let empty = Value::Array(Vec::new());
    let key = "$generic$Yu79T6tn6ak4fFir9NQwWuuq_AgGQlEIAtzovkd_";
    let (good, untimed, minus) = (
        format!("4102444800:{key}").leak(),
        format!(":{key}").leak(),
        format!("-86400:{key}").leak(),
    );
    // Asked for a day before now, so it expires now; and a second later.
    let (now, later) = (
        format!("{}:{key}", NOW - 86_400).leak(),
        format!("{}:{key}", NOW - 86_399).leak(),
    );
    let cookie = b"admin:freshkey0123456789AB";
    let at = |stored: &'static str| Oracle {
        activation_key: stored.as_bytes(),
        ..Oracle::default()
    };
    let form = action(
        &at(later),
        &request(&empty, Some(cookie)),
        "rp",
        (b"/", NOW),
    );
    assert!(matches!(form, Action::Page(_)));
    for (stored, cookie, ours) in [
        (now.as_bytes(), &cookie[..], lost("expiredkey")),
        (untimed.as_bytes(), cookie, Action::NotPorted),
        (b"abc:x", cookie, Action::NotPorted),
        (minus.as_bytes(), cookie, lost("expiredkey")),
        (b"1700000000:", cookie, lost("invalidkey")),
        (b"1700000000:0", cookie, lost("invalidkey")),
        (
            good.as_bytes(),
            b"0:freshkey0123456789AB",
            lost("invalidkey"),
        ),
        (
            good.as_bytes(),
            b" :freshkey0123456789AB",
            lost("invalidkey"),
        ),
        (
            good.as_bytes(),
            b":freshkey0123456789AB",
            lost("invalidkey"),
        ),
        (good.as_bytes(), b"admin:-", lost("invalidkey")),
        (good.as_bytes(), b"admin:0", lost("invalidkey")),
        (
            good.as_bytes(),
            b"nobody:freshkey0123456789AB",
            lost("invalidkey"),
        ),
    ] {
        let oracle = Oracle {
            activation_key: stored,
            ..Oracle::default()
        };
        let ours_now = action(&oracle, &request(&empty, Some(cookie)), "rp", (b"/", NOW));
        assert_eq!(ours_now, ours, "{cookie:?}");
    }
    let oracle = Oracle {
        activation_key: good.as_bytes(),
        ..Oracle::default()
    };
    let rp_key = "rp_key=freshkey0123456789AB";
    for (form, posted) in [
        ("pass1=x", None),
        ("pass1=x&rp_key[]=x", None),
        (&format!("pass1[]=x&{rp_key}"), None),
        (&format!("pass1=x&pass2[]=x&{rp_key}"), None),
        (&format!("pass1[]=&{rp_key}"), None),
        (
            &format!("pass1=%200%20&pass2=0&{rp_key}"),
            Some("all spaces"),
        ),
    ] {
        let form = parse_str(form.as_bytes());
        let request = Request {
            form: Some(&form),
            ..request(&empty, Some(cookie))
        };
        match (action(&oracle, &request, "resetpass", (b"/", NOW)), posted) {
            (Action::NotPorted, None) => {}
            (Action::Page(page), Some(text)) => {
                assert!(String::from_utf8(page).unwrap().contains(text));
            }
            (ours, _) => panic!("{form:?}: {ours:?}"),
        }
    }
    let oracle = Oracle {
        user_request: Some(7),
        ..Oracle::default()
    };
    for (query, name) in [("key%5B%5D=x&login=admin", "rp"), ("", "postpass")] {
        let query = parse_str(query.as_bytes());
        assert_eq!(
            action(&oracle, &request(&query, None), name, (b"/", NOW)),
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
        action(&oracle, &request(&query, None), "lostpassword", (b"/", NOW))
    };
    assert_eq!(
        page("action=lostpassword&redirect_to=0"),
        page("action=lostpassword")
    );
}

/// The confirmed page words each action as PHP does, and an action it has
/// no words for gets the general ones.
#[test]
fn the_confirmed_page_names_the_action() {
    let oracle = Oracle::default();
    let query = parse_str(b"");
    for (name, text) in [
        (
            &b"export_personal_data"[..],
            "Thanks for confirming your export request.",
        ),
        (
            b"remove_personal_data",
            "Thanks for confirming your erasure request.",
        ),
        (b"other", "Action has been confirmed."),
    ] {
        let page = request_confirmed(&oracle, &request(&query, None), name);
        let page = String::from_utf8(page).unwrap();
        let paragraph = format!("<p class=\"success\">{text}</p>");
        assert!(page.contains(&paragraph), "{text}");
    }
}

/// A confirmation key is good for a day to the second.
#[test]
fn a_confirmation_key_lasts_a_day() {
    let oracle = Oracle {
        user_request: Some(7),
        ..Oracle::default()
    };
    let query = parse_str(b"request_id=7&confirm_key=a-key");
    let at = |now| {
        action(
            &oracle,
            &request(&query, None),
            "confirmaction",
            (b"/", now),
        )
    };
    let last = REQUESTED + 86_400;
    assert!(matches!(at(last), Action::ConfirmRequest(found, 7) if found.id == 7));
    assert!(matches!(at(last + 1), Action::Died(_)));
}
