// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn owned(value: &[u8]) -> Option<Vec<u8>> {
    Some(value.to_vec())
}

#[test]
fn the_headers_are_read_as_wp_mail_reads_them() {
    let headers = b"From: \"A \"Reader\"\" <wordpress@example.test>\r\n\
        Content-Type: text/html; charset=\"ISO-8859-1\"; format=flowed\n\
        Reply-To: \"A\" <a@example.test>, b@example.test\n\
        reply-to: c@example.test\n\
        Cc: c@example.test\nBcc: d@example.test\nX-Mailer: Something\n\
        boundary line without a colon\n\n";
    let parsed = parse(headers).unwrap();
    assert_eq!(
        parsed,
        Headers {
            from_name: owned(b"A Reader"),
            from_email: owned(b"wordpress@example.test"),
            content_type: owned(b"text/html"),
            charset: owned(b"ISO-8859-1"),
            reply_to: vec![
                b"\"A\" <a@example.test>".to_vec(),
                b" b@example.test".to_vec(),
                b"c@example.test".to_vec()
            ],
            dropped: vec![b"Cc".to_vec(), b"Bcc".to_vec(), b"X-Mailer".to_vec()],
        }
    );
    assert_eq!(parse(b""), Some(Headers::default()));
}

#[test]
fn from_takes_a_name_only_before_a_bracket() {
    let from = |value: &[u8]| {
        let parsed = parse(&[&b"From: "[..], value].concat()).unwrap();
        (parsed.from_name, parsed.from_email)
    };
    assert_eq!(from(b"<a@example.test>"), (None, owned(b"a@example.test")));
    assert_eq!(from(b"a@example.test"), (None, owned(b"a@example.test")));
    assert_eq!(
        from(b"Site < a@example.test > "),
        (owned(b"Site"), owned(b"a@example.test"))
    );
    assert_eq!(from(b""), (None, None));
}

#[test]
fn content_type_takes_a_charset_only_when_named() {
    let kind = |value: &[u8]| {
        let parsed = parse(&[&b"Content-Type: "[..], value].concat()).unwrap();
        (parsed.content_type, parsed.charset)
    };
    assert_eq!(kind(b"text/plain"), (owned(b"text/plain"), None));
    assert_eq!(
        kind(b"text/plain; format=flowed"),
        (owned(b"text/plain"), None)
    );
    // `stripos()` finds it in any case; `str_replace()` removes only `charset=`.
    assert_eq!(
        kind(b"text/plain; Charset=utf-8"),
        (owned(b"text/plain"), owned(b"Charset=utf-8"))
    );
    assert_eq!(kind(b""), (None, None));
}

#[test]
fn a_stray_carriage_return_or_nul_refuses_the_headers() {
    assert!(parse(b"From: a@example.test\rBcc: b@example.test\n").is_none());
    assert!(parse(b"From: a@example.test\0\n").is_none());
    assert!(parse(b"From: a@example.test\r\nReply-To: b@example.test\r\n").is_some());
    assert!(breaks_line(b"a\nb") && breaks_line(b"a\rb") && breaks_line(b"a\0b"));
    assert!(!breaks_line(b"a\tb"));
}
