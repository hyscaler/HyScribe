// SPDX-License-Identifier: AGPL-3.0-or-later

//! The transport against a local SMTP sink on 127.0.0.1, plain, with
//! STARTTLS or with TLS from the first byte, under the fetch client's
//! test certificates for `pages.test`; no network.

use std::sync::Arc;

use hyscribe_config::{Secret, Smtp, SmtpTls};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::*;
use crate::hooks::NoHooks;

/// What the sink was told: every command line, and each message's data.
#[derive(Default)]
struct Told {
    lines: Vec<String>,
    data: Vec<String>,
}

/// How the sink speaks.
#[derive(Clone, Copy, PartialEq)]
enum Sink {
    /// Plain text, offering nothing.
    Plain,
    /// STARTTLS and AUTH offered, TLS after STARTTLS.
    StartTls,
    /// TLS from the first byte, AUTH offered.
    Wrapped,
    /// Accepts and says nothing.
    Silent,
}

trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}

fn acceptor() -> tokio_rustls::TlsAcceptor {
    let leaf = include_bytes!("../../../hyscribe-fetch/tests/tls/leaf.pem");
    let key = include_bytes!("../../../hyscribe-fetch/tests/tls/leaf-key.pem");
    let chain = CertificateDer::pem_slice_iter(leaf)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(key).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}

/// The test CA, which signed `pages.test`'s certificate, as DER.
fn ca() -> Vec<Vec<u8>> {
    let pem = include_bytes!("../../../hyscribe-fetch/tests/tls/ca.pem");
    vec![CertificateDer::from_pem_slice(pem).unwrap().to_vec()]
}

/// A sink speaking `mode`, answering `RCPT` with `rcpt`: its port and
/// what it was told.
async fn sink(mode: Sink, rcpt: &'static str) -> (u16, Arc<Mutex<Told>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let told = Arc::new(Mutex::new(Told::default()));
    let kept = Arc::clone(&told);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let told = Arc::clone(&kept);
            tokio::spawn(async move {
                let stream: Box<dyn Duplex> = match mode {
                    Sink::Wrapped => match acceptor().accept(stream).await {
                        Ok(stream) => Box::new(stream),
                        // A client that refuses the certificate.
                        Err(_) => return,
                    },
                    Sink::Silent => {
                        tokio::time::sleep(Duration::from_secs(3600)).await;
                        return;
                    }
                    _ => Box::new(stream),
                };
                converse(stream, mode, rcpt, &told).await;
            });
        }
    });
    (port, told)
}

/// One SMTP conversation.
async fn converse(stream: Box<dyn Duplex>, mode: Sink, rcpt: &str, told: &Mutex<Told>) {
    let mut stream = BufReader::new(stream);
    let mut secured = mode == Sink::Wrapped;
    stream.write_all(b"220 sink ESMTP\r\n").await.unwrap();
    let mut line = String::new();
    loop {
        line.clear();
        if stream.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        let command = line.trim_end().to_string();
        told.lock().unwrap().lines.push(command.clone());
        let verb = command
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reply = match verb.as_str() {
            "EHLO" if mode == Sink::StartTls && !secured => "250-sink\r\n250 STARTTLS\r\n",
            "EHLO" if secured => "250-sink\r\n250 AUTH PLAIN LOGIN\r\n",
            "STARTTLS" => {
                stream.write_all(b"220 go ahead\r\n").await.unwrap();
                let plain = stream.into_inner();
                let secure: Box<dyn Duplex> = Box::new(acceptor().accept(plain).await.unwrap());
                stream = BufReader::new(secure);
                secured = true;
                continue;
            }
            "AUTH" => "235 ok\r\n",
            "RCPT" => rcpt,
            "DATA" => {
                stream.write_all(b"354 go ahead\r\n").await.unwrap();
                let mut data = String::new();
                while data.is_empty() || !data.ends_with("\r\n.\r\n") {
                    stream.read_line(&mut data).await.unwrap();
                }
                told.lock().unwrap().data.push(data);
                "250 2.0.0 queued\r\n"
            }
            "QUIT" => {
                let _ = stream.write_all(b"221 bye\r\n").await;
                return;
            }
            _ => "250 ok\r\n",
        };
        stream.write_all(reply.as_bytes()).await.unwrap();
    }
}

fn smtp(port: u16, tls: SmtpTls, login: bool) -> Smtp {
    Smtp {
        host: "127.0.0.1".into(),
        port,
        tls,
        login: login.then(|| ("me".to_string(), Secret::new("pw"))),
    }
}

fn plain(port: u16) -> hyscribe_config::Mail {
    hyscribe_config::Mail {
        smtp: Some(smtp(port, SmtpTls::None, false)),
        from: Some("site@example.test".into()),
    }
}

/// A mailer on `mail` for `http://www.example.test`.
fn mailer(mail: &hyscribe_config::Mail, hooks: Arc<dyn Hooks>) -> Mailer {
    let (mailer, _) = Mailer::start(mail, b"http://www.example.test/", hooks, &[], LIMITS);
    mailer
}

fn mail(to: &[u8], subject: &[u8], headers: &[u8]) -> Mail {
    Mail {
        to: to.to_vec(),
        subject: subject.to_vec(),
        message: b"Line one\r\nline two\n".to_vec(),
        headers: headers.to_vec(),
    }
}

/// A golden field as `run.php` printed it.
fn unescape(field: &str) -> Vec<u8> {
    let mut out = String::new();
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => out.push(match chars.next().unwrap() {
                't' => '\t',
                'n' => '\n',
                'r' => '\r',
                c => c,
            }),
            (c, false) => out.push(c),
        }
    }
    out.into_bytes()
}

/// The values of header `name` in a message's data, unfolded, with each
/// RFC 2047 word decoded.
fn header(data: &str, name: &str) -> Option<String> {
    let head = data.split("\r\n\r\n").next().unwrap().replace("\r\n ", " ");
    let line = head
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}: ")))?;
    let mut decoded = String::new();
    for word in line.split(' ') {
        match word
            .strip_prefix("=?utf-8?b?")
            .and_then(|w| w.strip_suffix("?="))
        {
            Some(encoded) => decoded.push_str(&String::from_utf8(base64(encoded)).unwrap()),
            None => {
                if !decoded.is_empty() {
                    decoded.push(' ');
                }
                decoded.push_str(word);
            }
        }
    }
    Some(decoded)
}

fn base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for c in text.bytes().filter(|&c| c != b'=') {
        bits = bits << 6 | ALPHABET.iter().position(|&a| a == c).unwrap() as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out
}

/// A message's body, decoded.
fn body(data: &str) -> Vec<u8> {
    let (_, body) = data.split_once("\r\n\r\n").unwrap();
    let body = body
        .strip_suffix("\r\n.\r\n")
        .unwrap()
        .replace("\r\n..", "\r\n.");
    quoted_printable::decode(body, quoted_printable::ParseMode::Strict).unwrap()
}

/// Lone line feeds as CRLF, as the library sends every line.
fn crlf(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, &b) in text.iter().enumerate() {
        if b == b'\n' && (i == 0 || text.get(i - 1) != Some(&b'\r')) {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

/// Every message PHP's `wp_mail()` was handed (`reference/mail/`) leaves
/// with its recipient, subject, body, sender's name and `Reply-To`.
#[tokio::test]
async fn the_golden_messages_reach_the_relay() {
    let (port, told) = sink(Sink::Plain, "250 ok\r\n").await;
    let mailer = mailer(&plain(port), Arc::new(NoHooks));
    let golden = include_str!("../../tests/golden/mail.tsv");
    let mut sent = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [_, to, subject, message, headers] = fields[..] else {
            continue;
        };
        let mail = Mail {
            to: unescape(to),
            subject: unescape(subject),
            message: unescape(message),
            headers: unescape(headers),
        };
        assert!(mailer.wp_mail(mail.clone(), "golden", true).await, "{line}");
        let told = told.lock().unwrap();
        let data = told.data.last().unwrap();
        let address = String::from_utf8(mail.to.clone()).unwrap();
        assert_eq!(
            header(data, "To").unwrap().trim_matches(['<', '>']),
            address
        );
        assert_eq!(
            header(data, "Subject").unwrap().into_bytes(),
            mail.subject,
            "{line}"
        );
        assert_eq!(body(data), crlf(&mail.message), "{line}");
        let parsed = parse(&mail.headers).unwrap();
        let name = parsed.from_name.unwrap_or_else(|| b"WordPress".to_vec());
        let from = shown(&Recipient {
            name: String::from_utf8(name).unwrap(),
            address: "site@example.test".into(),
        });
        assert_eq!(header(data, "From").unwrap(), from, "{line}");
        let reply_to: Vec<String> = parsed
            .reply_to
            .iter()
            .map(|r| shown(&recipient(r).unwrap()))
            .collect();
        let reply_to = Some(reply_to.join(", ")).filter(|r| !r.is_empty());
        assert_eq!(header(data, "Reply-To"), reply_to, "{line}");
        assert!(
            told.lines
                .contains(&"MAIL FROM:<site@example.test>".to_string())
        );
        assert!(told.lines.contains(&format!("RCPT TO:<{address}>")));
        sent += 1;
    }
    assert_eq!(sent, 83);
    assert!(
        told.lock()
            .unwrap()
            .lines
            .contains(&"EHLO www.example.test".to_string())
    );
}

/// A recipient as the library writes it.
fn shown(recipient: &Recipient) -> String {
    let address = recipient.address.parse().unwrap();
    lettre::message::Mailbox::new(Some(recipient.name.clone()), address).to_string()
}

/// Every hook in order, recording what it was given.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<String>>,
    pre: Option<bool>,
    init: Option<bool>,
    from_name: Option<&'static [u8]>,
    content_type: Option<&'static [u8]>,
    charset: Option<&'static [u8]>,
    sent: Mutex<Vec<Outgoing>>,
}

impl Recorder {
    fn call(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Hooks for Recorder {
    fn wp_mail(&self, mut atts: Mail) -> Mail {
        self.call("wp_mail".into());
        atts.subject.extend_from_slice(b" (filtered)");
        atts
    }
    fn pre_wp_mail(&self, atts: &Mail) -> Option<bool> {
        self.call(format!(
            "pre_wp_mail {}",
            String::from_utf8_lossy(&atts.subject)
        ));
        self.pre
    }
    fn wp_mail_from(&self, from_email: Vec<u8>) -> Vec<u8> {
        self.call(format!(
            "wp_mail_from {}",
            String::from_utf8_lossy(&from_email)
        ));
        b"other@example.test".to_vec()
    }
    fn wp_mail_from_name(&self, from_name: Vec<u8>) -> Vec<u8> {
        self.call(format!(
            "wp_mail_from_name {}",
            String::from_utf8_lossy(&from_name)
        ));
        self.from_name.map_or(from_name, <[u8]>::to_vec)
    }
    fn wp_mail_content_type(&self, content_type: Vec<u8>) -> Vec<u8> {
        self.call(format!(
            "wp_mail_content_type {}",
            String::from_utf8_lossy(&content_type)
        ));
        self.content_type.map_or(content_type, <[u8]>::to_vec)
    }
    fn wp_mail_charset(&self, charset: Vec<u8>) -> Vec<u8> {
        self.call(format!(
            "wp_mail_charset {}",
            String::from_utf8_lossy(&charset)
        ));
        self.charset.map_or(charset, <[u8]>::to_vec)
    }
    fn phpmailer_init(&self, message: &Outgoing) -> Option<bool> {
        self.call("phpmailer_init".into());
        self.sent.lock().unwrap().push(message.clone());
        self.init
    }
    fn wp_mail_succeeded(&self, mail_data: &Mail) {
        self.call(format!(
            "wp_mail_succeeded {}",
            String::from_utf8_lossy(&mail_data.subject)
        ));
    }
    fn wp_mail_failed(&self, _: &Mail, error: &str) {
        self.call(format!("wp_mail_failed {error}"));
    }
}

#[tokio::test]
async fn the_filters_run_in_wordpress_order() {
    let (port, told) = sink(Sink::Plain, "250 ok\r\n").await;
    let hooks = Arc::new(Recorder::default());
    let mailer = mailer(&plain(port), hooks.clone());
    let headers = b"From: \"Me\" <me@example.test>\nContent-Type: text/plain; charset=utf-8\n";
    assert!(
        mailer
            .wp_mail(mail(b"a@example.test", b"Hi", headers), "test", true)
            .await
    );
    assert_eq!(
        hooks.calls(),
        [
            "wp_mail",
            "pre_wp_mail Hi (filtered)",
            "wp_mail_from me@example.test",
            "wp_mail_from_name Me",
            "wp_mail_content_type text/plain",
            "wp_mail_charset utf-8",
            "phpmailer_init",
            "wp_mail_succeeded Hi (filtered)",
        ]
    );
    // The address is `MAIL_FROM`'s whatever the header and the filter say.
    let out = hooks.sent.lock().unwrap()[0].clone();
    let to = vec![Recipient {
        name: String::new(),
        address: "a@example.test".into(),
    }];
    let wanted = Outgoing {
        from: "site@example.test".into(),
        from_name: "Me".into(),
        to,
        reply_to: Vec::new(),
        subject: "Hi (filtered)".into(),
        body: "Line one\r\nline two\n".into(),
        html: false,
    };
    assert_eq!(out, wanted);
    let data = told.lock().unwrap().data[0].clone();
    assert!(data.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    let id = header(&data, "Message-ID").unwrap();
    assert!(
        id.starts_with('<') && id.ends_with(".0@example.test>"),
        "{id}"
    );
}

#[tokio::test]
async fn a_hook_may_answer_for_the_transport() {
    let sent = |pre, init| async move {
        let hooks = Arc::new(Recorder {
            pre,
            init,
            ..Recorder::default()
        });
        let mailer = mailer(&hyscribe_config::Mail::default(), hooks.clone());
        let result = mailer
            .wp_mail(mail(b"a@example.test", b"Hi", b""), "test", true)
            .await;
        (result, hooks.calls().last().unwrap().clone())
    };
    assert_eq!(
        sent(Some(true), None).await,
        (true, "pre_wp_mail Hi (filtered)".into())
    );
    assert_eq!(
        sent(Some(false), None).await,
        (false, "pre_wp_mail Hi (filtered)".into())
    );
    let succeeded = "wp_mail_succeeded Hi (filtered)".to_string();
    assert_eq!(sent(None, Some(true)).await, (true, succeeded));
    let failed = "wp_mail_failed not sent by phpmailer_init".to_string();
    assert_eq!(sent(None, Some(false)).await, (false, failed));
    // With no hook to send it and no relay, nothing is sent.
    let off = "wp_mail_failed mail is off".to_string();
    assert_eq!(sent(None, None).await, (false, off));
}

#[tokio::test]
async fn html_is_sent_as_html_and_other_types_are_refused() {
    let (port, told) = sink(Sink::Plain, "250 ok\r\n").await;
    let refusal = |hooks: Recorder| async {
        let hooks = Arc::new(hooks);
        let mailer = mailer(&plain(port), hooks.clone());
        let sent = mailer
            .wp_mail(mail(b"a@example.test", b"Hi", b""), "test", true)
            .await;
        (sent, hooks.calls().last().unwrap().clone())
    };
    let html = Recorder {
        content_type: Some(b"text/html"),
        ..Recorder::default()
    };
    assert!(refusal(html).await.0);
    let data = told.lock().unwrap().data[0].clone();
    assert!(data.contains("Content-Type: text/html; charset=utf-8\r\n"));
    let other = Recorder {
        content_type: Some(b"multipart/alternative"),
        ..Recorder::default()
    };
    let why = "wp_mail_failed refused: a content type other than text/plain or text/html";
    assert_eq!(refusal(other).await, (false, why.into()));
    let latin = Recorder {
        charset: Some(b"ISO-8859-1"),
        ..Recorder::default()
    };
    let why = "wp_mail_failed refused: a charset other than UTF-8";
    assert_eq!(refusal(latin).await, (false, why.into()));
    let empty_name = Recorder {
        from_name: Some(b""),
        ..Recorder::default()
    };
    assert!(refusal(empty_name).await.0);
    let data = told.lock().unwrap().data[1].clone();
    assert!(data.contains("\r\nFrom: site@example.test\r\n"), "{data}");
    let injected = Recorder {
        from_name: Some(b"Me\r\nBcc: victim@example.test"),
        ..Recorder::default()
    };
    let why = "wp_mail_failed refused: a line break in the sender's name";
    assert_eq!(refusal(injected).await, (false, why.into()));
    assert_eq!(told.lock().unwrap().data.len(), 2);
}

/// A CR, LF or NUL anywhere a header is made refuses the message, which
/// never reaches the relay.
#[tokio::test]
async fn header_injection_is_refused() {
    let (port, told) = sink(Sink::Plain, "250 ok\r\n").await;
    let hooks = Arc::new(Recorder::default());
    let mailer = mailer(&plain(port), hooks.clone());
    let cases: [(&[u8], &[u8], &[u8], &str); 8] = [
        (
            b"a@example.test",
            b"Hi\r\nBcc: v@example.test",
            b"",
            "a line break in the subject or recipient",
        ),
        (
            b"a@example.test",
            b"Hi\nBcc: v@example.test",
            b"",
            "a line break in the subject or recipient",
        ),
        (
            b"a@example.test",
            b"Hi\0",
            b"",
            "a line break in the subject or recipient",
        ),
        (
            b"a@example.test\r\nBcc: v@example.test",
            b"Hi",
            b"",
            "a line break in the subject or recipient",
        ),
        (
            b"a@example.test",
            b"Hi",
            b"From: Me\r<me@example.test>\n",
            "a line break in the headers",
        ),
        (
            b"a@example.test",
            b"Hi",
            b"Reply-To: r@example.test\0Bcc: v@example.test",
            "a line break in the headers",
        ),
        (
            b"a@example.test, not an address",
            b"Hi",
            b"",
            "an address the parser refuses",
        ),
        (
            b"\xff@example.test",
            b"Hi",
            b"",
            "an address that is not UTF-8",
        ),
    ];
    for (to, subject, headers, why) in cases {
        assert!(
            !mailer
                .wp_mail(mail(to, subject, headers), "test", true)
                .await
        );
        assert_eq!(
            hooks.calls().last().unwrap(),
            &format!("wp_mail_failed refused: {why}")
        );
    }
    let bad_reply_to = mail(b"a@example.test", b"Hi", b"Reply-To: \xff\n");
    assert!(!mailer.wp_mail(bad_reply_to, "test", true).await);
    let no_recipient = mail(b"", b"Hi", b"");
    assert!(!mailer.wp_mail(no_recipient, "test", true).await);
    assert!(told.lock().unwrap().data.is_empty());
    // A comma list and names reach the relay, Bcc dropped.
    let list = mail(
        b"A <a@example.test>, b@example.test",
        b"Hi",
        b"Bcc: v@example.test\n",
    );
    assert!(mailer.wp_mail(list, "test", true).await);
    let told = told.lock().unwrap();
    assert!(told.lines.contains(&"RCPT TO:<b@example.test>".to_string()));
    assert!(
        !told
            .lines
            .iter()
            .any(|line| line.contains("v@example.test"))
    );
    assert_eq!(
        header(&told.data[0], "To").unwrap(),
        "A <a@example.test>, b@example.test"
    );
}

#[test]
fn recipients_are_split_as_wp_mail_splits_them() {
    let recipient = |entry: &[u8]| recipient(entry).map(|r| (r.name, r.address));
    let pair = |name: &str, address: &str| Some((name.to_string(), address.to_string()));
    assert_eq!(
        recipient(b" A B <a@example.test> "),
        pair("A B", "a@example.test")
    );
    assert_eq!(recipient(b"<a@example.test>"), pair("", "a@example.test"));
    assert_eq!(recipient(b"a@example.test"), pair("", "a@example.test"));
    assert_eq!(recipient(b"a<>"), pair("", "a<>"));
    assert_eq!(recipient(b"a>b<"), pair("", "a>b<"));
    assert_eq!(recipient(b"\xff <a@example.test>"), None);
    assert_eq!(
        domain(b"A <a@Mail.Example-1.test>, b@c.test"),
        "Mail.Example-1.test"
    );
    assert_eq!(domain(b"a@b.test\r\nBcc: c"), "b.test");
    assert_eq!(domain(b"nobody"), "");
}

#[tokio::test]
async fn the_relay_takes_starttls_tls_and_a_login() {
    let message = |mailer: &Mailer| {
        let atts = mail(b"a@example.test", b"Hi", b"");
        mailer.prepare(&atts, "test").unwrap().1
    };
    let off = mailer(&hyscribe_config::Mail::default(), Arc::new(NoHooks));
    for (mode, tls) in [
        (Sink::StartTls, SmtpTls::StartTls),
        (Sink::Wrapped, SmtpTls::Tls),
    ] {
        let (port, told) = sink(mode, "250 ok\r\n").await;
        let relay =
            Relay::new(&smtp(port, tls, true), "pages.test", &ca(), "example.test").unwrap();
        assert_eq!(relay.deliver(&message(&off)).await, Ok("250".into()));
        let told = told.lock().unwrap();
        // PLAIN of `me` and `pw`, only over TLS.
        assert!(
            told.lines.contains(&"AUTH PLAIN AG1lAHB3".to_string()),
            "{:?}",
            told.lines
        );
        assert_eq!(
            told.lines.contains(&"STARTTLS".to_string()),
            mode == Sink::StartTls
        );
    }
    // STARTTLS is required: a relay without it gets nothing.
    let (port, told) = sink(Sink::Plain, "250 ok\r\n").await;
    let relay = Relay::new(
        &smtp(port, SmtpTls::StartTls, true),
        "pages.test",
        &ca(),
        "example.test",
    );
    let failed = relay.unwrap().deliver(&message(&off)).await;
    assert_eq!(failed, Err("connection failed, no reply".into()));
    assert!(
        !told
            .lock()
            .unwrap()
            .lines
            .iter()
            .any(|line| line.starts_with("AUTH"))
    );
    // The certificate must be the relay's name's.
    let (port, _) = sink(Sink::Wrapped, "250 ok\r\n").await;
    let relay = Relay::new(
        &smtp(port, SmtpTls::Tls, false),
        "other.test",
        &ca(),
        "example.test",
    );
    let failed = relay.unwrap().deliver(&message(&off)).await;
    assert_eq!(failed, Err("connection failed, no reply".into()));
}

#[tokio::test]
async fn relay_errors_are_logged_by_code_and_kind() {
    let off = mailer(&hyscribe_config::Mail::default(), Arc::new(NoHooks));
    let message = off
        .prepare(&mail(b"a@example.test", b"Hi", b""), "test")
        .unwrap()
        .1;
    for (rcpt, wanted) in [
        ("550 5.1.1 no such user\r\n", "refused, 550"),
        ("451 4.3.0 try later\r\n", "deferred, 451"),
    ] {
        let (port, _) = sink(Sink::Plain, rcpt).await;
        let relay = Relay::new(&smtp(port, SmtpTls::None, false), "", &[], "example.test").unwrap();
        assert_eq!(relay.deliver(&message).await, Err(wanted.into()));
    }
    let (port, _) = sink(Sink::Silent, "").await;
    let mut relay = Relay::new(&smtp(port, SmtpTls::None, false), "", &[], "example.test").unwrap();
    relay.limit = Duration::from_millis(100);
    assert_eq!(relay.deliver(&message).await, Err("timed out".into()));
    let junk = Relay::new(
        &smtp(port, SmtpTls::Tls, false),
        "pages.test",
        &[b"junk".to_vec()],
        "x",
    );
    assert!(junk.is_err());
}

#[tokio::test]
async fn the_start_log_says_whether_mail_is_on() {
    let hooks = || -> Arc<dyn Hooks> { Arc::new(NoHooks) };
    let (off, line) = Mailer::new(
        &hyscribe_config::Mail::default(),
        b"http://www.example.test",
        hooks(),
    );
    assert_eq!(line, "mail off: SMTP_HOST is not set");
    assert_eq!(format!("{off:?}"), "Mailer { on: false, .. }");
    assert_eq!(off.from, "wordpress@example.test");
    let tls = hyscribe_config::Mail {
        smtp: Some(smtp(465, SmtpTls::Tls, false)),
        from: None,
    };
    let start =
        |roots: &[Vec<u8>]| Mailer::start(&tls, b"http://example.test", hooks(), roots, LIMITS);
    let line = "mail off: no certificate store was found to check the relay's";
    assert_eq!(start(&[]).1, line);
    assert!(
        start(&[b"junk".to_vec()])
            .1
            .starts_with("mail off: the relay's TLS: ")
    );
    let (on, line) = start(&ca());
    assert_eq!(line, "mail on through 127.0.0.1:465");
    assert_eq!(format!("{on:?}"), "Mailer { on: true, .. }");
    // A site without a host greets as `localhost`, and has no sender.
    let (port, told) = sink(Sink::Plain, "250 ok\r\n").await;
    let none = hyscribe_config::Mail {
        smtp: Some(smtp(port, SmtpTls::None, false)),
        from: None,
    };
    let (bare, _) = Mailer::start(&none, b"", hooks(), &[], LIMITS);
    assert_eq!(bare.from, "wordpress@");
    assert!(
        !bare
            .wp_mail(mail(b"a@example.test", b"Hi", b""), "test", true)
            .await
    );
    let named = hyscribe_config::Mail {
        from: Some("site@example.test".into()),
        ..none
    };
    let (bare, _) = Mailer::start(&named, b"", hooks(), &[], LIMITS);
    let hi = mail(b"a@example.test", b"Hi", b"");
    assert!(bare.wp_mail(hi, "test", true).await);
    assert!(
        told.lock()
            .unwrap()
            .lines
            .contains(&"EHLO localhost".to_string())
    );
}

/// Past the rate messages wait; a full queue refuses, and a caller that
/// cannot wait longer counts its message as not sent, which is dropped.
#[tokio::test]
async fn the_queue_is_bounded_and_rate_limited() {
    let (port, told) = sink(Sink::Plain, "250 ok\r\n").await;
    let hooks = Arc::new(Recorder::default());
    let limits = Limits {
        per_window: 1,
        window: Duration::from_millis(500),
        queue: 1,
        wait: Duration::from_millis(100),
    };
    let home = b"http://example.test";
    let (mailer, _) = Mailer::start(&plain(port), home, hooks.clone(), &[], limits);
    let send = |wait| mailer.wp_mail(mail(b"a@example.test", b"Hi", b""), "test", wait);
    let pause = |ms| tokio::time::sleep(Duration::from_millis(ms));
    assert!(send(true).await);
    // The relay's task takes this one and waits out the window.
    assert!(send(false).await);
    pause(50).await;
    assert!(send(false).await);
    assert!(!send(false).await);
    assert_eq!(
        hooks.calls().last().unwrap(),
        "wp_mail_failed the queue is full"
    );
    pause(600).await;
    assert!(!send(true).await);
    pause(900).await;
    assert_eq!(
        hooks.calls().last().unwrap(),
        "wp_mail_failed not sent in time"
    );
    assert_eq!(told.lock().unwrap().data.len(), 3);
}

#[tokio::test]
async fn password_resets_are_limited() {
    let hooks = Arc::new(Recorder {
        init: Some(true),
        ..Recorder::default()
    });
    let mailer = mailer(&hyscribe_config::Mail::default(), hooks.clone());
    let address: IpAddr = "192.0.2.1".parse().unwrap();
    let reset = |account| {
        mailer.retrieve_password(mail(b"a@example.test", b"Hi", b""), account, Some(address))
    };
    for _ in 0..3 {
        assert!(reset(1).await);
    }
    assert!(!reset(1).await);
    for account in 2..9 {
        assert!(reset(account).await);
    }
    assert!(!reset(9).await);
    let elsewhere = mailer.retrieve_password(mail(b"a@example.test", b"Hi", b""), 9, None);
    assert!(elsewhere.await);
    assert_eq!(hooks.sent.lock().unwrap().len(), 11);
}

#[test]
fn reset_counts_last_an_hour_and_are_bounded() {
    let mut resets = Resets::default();
    let start = Instant::now();
    let address: IpAddr = "192.0.2.1".parse().unwrap();
    for _ in 0..3 {
        assert_eq!(resets.allow(1, Some(address), start), Ok(()));
    }
    let account = "the account's limit of 3 password resets an hour";
    assert_eq!(
        resets.allow(1, None, start + HOUR - Duration::from_secs(1)),
        Err(account)
    );
    assert_eq!(resets.allow(1, None, start + HOUR), Ok(()));
    assert_eq!(resets.accounts.len(), 1);
    assert!(resets.addresses.is_empty());
    let later = start + HOUR * 2;
    for account in 0..TRACKED as u64 {
        resets.accounts.insert(account, vec![later]);
    }
    let full = "too many password resets in the hour to track";
    assert_eq!(resets.allow(u64::MAX, None, later), Err(full));
    resets.accounts.clear();
    for n in 0..TRACKED {
        resets
            .addresses
            .insert(IpAddr::from([10, 0, (n >> 8) as u8, n as u8]), vec![later]);
    }
    assert_eq!(resets.allow(1, None, later), Err(full));
    assert_eq!(resets.allow(1, None, later + HOUR), Ok(()));
}

#[test]
fn the_hooks_default_to_their_value() {
    let atts = mail(b"a@example.test", b"Hi", b"");
    assert_eq!(NoHooks.wp_mail(atts.clone()), atts);
    assert_eq!(NoHooks.pre_wp_mail(&atts), None);
    assert_eq!(NoHooks.wp_mail_from(b"a".to_vec()), b"a");
    assert_eq!(NoHooks.wp_mail_from_name(b"b".to_vec()), b"b");
    assert_eq!(NoHooks.wp_mail_content_type(b"c".to_vec()), b"c");
    assert_eq!(NoHooks.wp_mail_charset(b"d".to_vec()), b"d");
    let off = mailer(&hyscribe_config::Mail::default(), Arc::new(NoHooks));
    let out = off.prepare(&atts, "test").unwrap().0;
    assert_eq!(NoHooks.phpmailer_init(&out), None);
    NoHooks.wp_mail_succeeded(&atts);
    NoHooks.wp_mail_failed(&atts, "why");
}
