// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Value;
use hyscribe_testkit::hooks::recorded;

use super::*;

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

#[test]
fn the_sender_s_filters_reach_the_extensions() {
    let answers = [
        ("wp_mail_from", text("from@example.test")),
        ("wp_mail_from_name", text("Shop")),
        ("wp_mail_content_type", text("text/html")),
        ("wp_mail_charset", text("ISO-8859-1")),
    ];
    let (got, fired) = recorded(&answers, || {
        [
            NoHooks.wp_mail_from(b"wordpress@localhost".to_vec()),
            NoHooks.wp_mail_from_name(b"WordPress".to_vec()),
            NoHooks.wp_mail_content_type(b"text/plain".to_vec()),
            NoHooks.wp_mail_charset(b"UTF-8".to_vec()),
        ]
    });
    let answered: [&[u8]; 4] = [b"from@example.test", b"Shop", b"text/html", b"ISO-8859-1"];
    assert_eq!(got, answered);
    let names: Vec<_> = fired
        .iter()
        .map(|(hook, value, args)| (hook.as_str(), value.clone(), args.len()))
        .collect();
    let expected = [
        ("wp_mail_from", text("wordpress@localhost"), 0),
        ("wp_mail_from_name", text("WordPress"), 0),
        ("wp_mail_content_type", text("text/plain"), 0),
        ("wp_mail_charset", text("UTF-8"), 0),
    ];
    assert_eq!(names, expected);
}
