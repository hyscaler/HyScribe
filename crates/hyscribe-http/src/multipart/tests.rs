// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn boundaries_are_read_from_the_content_type() {
    let cases: [(&[u8], Option<&[u8]>); 9] = [
        (b"multipart/form-data; boundary=abc", Some(b"abc")),
        (b"Multipart/Form-Data;boundary=\"a b;c\"", Some(b"a b;c")),
        (
            b"multipart/form-data; charset=x; BOUNDARY = xyz ; q=1",
            Some(b"xyz"),
        ),
        (b"multipart/form-data", None),
        (b"multipart/form-data; boundary=", None),
        (b"multipart/form-data; boundary", None),
        (b"multipart/mixed; boundary=abc", None),
        (b"text/plain; boundary=abc", None),
        (b"multipart/form-data; x=\"unclosed", None),
    ];
    for (content_type, boundary_found) in cases {
        assert_eq!(boundary(content_type), boundary_found, "{content_type:?}");
    }
    let long = [&b"multipart/form-data; boundary="[..], &[b'x'; 71]].concat();
    assert_eq!(boundary(&long), None);
    assert_eq!(boundary(&long[..long.len() - 1]), Some(&[b'x'; 70][..]));
}

/// A part with `headers` and `body`.
fn part(headers: &str, body: &[u8]) -> Vec<u8> {
    [b"--B\r\n", headers.as_bytes(), b"\r\n\r\n", body, b"\r\n"].concat()
}

/// The file of the field `field`, if one was sent.
fn find<'a>(form: &'a Form<'_>, field: &str) -> Option<&'a File<'a>> {
    form.files
        .iter()
        .find(|file| file.field == field.as_bytes())
}

#[test]
fn forms_give_fields_and_files() {
    let body = [
        &b"preamble\r\n"[..],
        &part("Content-Disposition: form-data; name=\"action\"", b"upload-attachment"),
        &part(
            "content-disposition: form-data; name=\"async-upload\"; filename=\"C:\\\\dir\\\\a b.jpg\"; size=3\r\nContent-Type: image/jpeg\r\nX-Other: 1\r\nNo colon",
            b"\xff\xd8bytes\r\n--C",
        ),
        &part(
            "Content-Disposition: form-data; name=post_data[context]",
            b"x&y=z",
        ),
        &part(
            "Content-Disposition: form-data; filename=\"dir/n.png\"; name=\"file\"",
            b"",
        ),
        b"--B--\r\nepilogue",
    ]
    .concat();
    let form = read(&body, b"B").unwrap();
    assert_eq!(
        form.fields,
        [
            (b"action".to_vec(), &b"upload-attachment"[..]),
            (b"post_data[context]".to_vec(), &b"x&y=z"[..]),
        ]
    );
    let file = find(&form, "async-upload").unwrap();
    assert_eq!(file.name, b"a b.jpg");
    assert_eq!(file.kind, b"image/jpeg");
    assert_eq!(file.bytes, b"\xff\xd8bytes\r\n--C");
    let other = find(&form, "file").unwrap();
    assert_eq!(
        (&other.name[..], &other.kind[..], other.bytes),
        (&b"n.png"[..], &b""[..], &b""[..])
    );
    assert!(find(&form, "missing").is_none());
    let post = form.post();
    assert_eq!(
        post.get_str("action"),
        Some(&Value::String(b"upload-attachment".to_vec()))
    );
    assert_eq!(
        post.get_path(&["post_data", "context"]),
        Some(&Value::String(b"x&y=z".to_vec()))
    );
}

/// A NUL ends a part's header line, as it ends PHP's C string: the file
/// name stops there, unquoted, and so does a type.
#[test]
fn a_nul_ends_a_header_line() {
    let body = [
        &part(
            "Content-Disposition: form-data; name=\"file\"; filename=\"a\0b.jpg\"\r\nContent-Type: image/jpeg\0x",
            b"bytes",
        ),
        &b"--B--\r\n"[..],
    ]
    .concat();
    let form = read(&body, b"B").unwrap();
    let file = find(&form, "file").unwrap();
    assert_eq!(
        (&file.name[..], &file.kind[..]),
        (&b"a"[..], &b"image/jpeg"[..])
    );
    assert_eq!(file.bytes, b"bytes");
}

#[test]
fn files_past_the_limit_are_dropped() {
    let mut body = Vec::new();
    for n in 0..22 {
        let headers = format!("Content-Disposition: form-data; name=\"f{n}\"; filename=\"{n}\"");
        body.extend(part(&headers, b"x"));
    }
    body.extend_from_slice(b"--B--");
    let form = read(&body, b"B").unwrap();
    assert_eq!(form.files.len(), 20);
    assert_eq!(form.files[19].field, b"f19");
}

#[test]
fn bodies_it_cannot_read_are_none() {
    let named = "Content-Disposition: form-data; name=\"a\"";
    let cases: [&[u8]; 6] = [
        b"no boundary at all",
        b"--Bjunk",
        b"--B\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nno end",
        b"--B\r\nno blank line\r\n--B--",
        &part("Content-Disposition: form-data", b"nameless"),
        &[&part(named, b"x")[..], b"--B"].concat(),
    ];
    for body in cases {
        let with_end = [body, b"--B--"].concat();
        let body = if body.ends_with(b"\r\n") {
            &with_end[..]
        } else {
            body
        };
        assert_eq!(
            read(body, b"B"),
            None,
            "{:?}",
            String::from_utf8_lossy(body)
        );
    }
    assert_eq!(read(b"--B--", b"B"), Some(Form::default()));
}

#[test]
fn disposition_filenames_are_php_s() {
    let cases: [(&[u8], Option<&[u8]>); 10] = [
        (b"attachment; filename=photo.jpg", Some(b"photo.jpg")),
        (b" attachment; filename=\"a b.jpg\" ", Some(b"a b.jpg")),
        (b"attachment;filename = x.png ; size=3", Some(b"x.png")),
        (
            b"attachment; filename=a.jpg; filename=b.jpg",
            Some(b"b.jpg"),
        ),
        (b"attachment; filename=\"q.jpg", Some(b"\"q.jpg")),
        (b"attachment; filename=\"\"", None),
        (b"attachment; filename=0", None),
        (b"attachment; filename=\"0\"", None),
        (b"filename=a.jpg", None),
        (b"attachment; name=a.jpg; nothing", None),
    ];
    for (value, filename) in cases {
        assert_eq!(
            disposition_filename(value).as_deref(),
            filename,
            "{value:?}"
        );
    }
}
