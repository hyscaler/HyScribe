// SPDX-License-Identifier: AGPL-3.0-or-later

//! The SMTP relay through `lettre` (ADR 0016), the only module that uses
//! it: the message built and encoded by the library, the relay's
//! certificate checked against the host's store for `SMTP_HOST`.

use std::time::Duration;

use hyscribe_config::{Smtp, SmtpTls};
use lettre::message::header::{ContentTransferEncoding, ContentType};
use lettre::message::{Mailbox, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Certificate, CertificateStore, Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::transport::smtp::{AsyncSmtpTransport, Error};
use lettre::{Address, AsyncTransport, Message, Tokio1Executor};

use crate::mailer::{Outgoing, Recipient};

/// The longest a whole SMTP conversation takes.
pub(crate) const LIMIT: Duration = Duration::from_secs(30);

/// The relay every message goes to.
pub(crate) struct Relay {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    /// The time a conversation may take.
    pub(crate) limit: Duration,
}

impl Relay {
    /// The relay of `smtp`, greeting it as `ehlo`; with TLS its
    /// certificate must be `name`'s, from one of `roots` (DER).
    pub(crate) fn new(
        smtp: &Smtp,
        name: &str,
        roots: &[Vec<u8>],
        ehlo: &str,
    ) -> Result<Relay, Error> {
        let parameters = || {
            let roots = roots
                .iter()
                .filter_map(|der| Certificate::from_der(der.clone()).ok());
            let builder = TlsParameters::builder(name.to_string());
            let builder = roots.fold(
                builder.certificate_store(CertificateStore::None),
                |b, root| b.add_root_certificate(root),
            );
            builder.build_rustls()
        };
        let tls = match smtp.tls {
            SmtpTls::StartTls => Tls::Required(parameters()?),
            SmtpTls::Tls => Tls::Wrapper(parameters()?),
            SmtpTls::None => Tls::None,
        };
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
            .port(smtp.port)
            .tls(tls)
            .hello_name(ClientId::Domain(ehlo.to_string()))
            .timeout(Some(LIMIT));
        if let Some((user, password)) = &smtp.login {
            let password = password.expose().to_string();
            builder = builder.credentials(Credentials::new(user.clone(), password));
        }
        Ok(Relay {
            transport: builder.build(),
            limit: LIMIT,
        })
    }

    /// Sends `message`: the relay's reply code, else the error's code and
    /// kind, never its text, which can echo the message.
    pub(crate) async fn deliver(&self, message: &Message) -> Result<String, String> {
        match tokio::time::timeout(self.limit, self.transport.send(message.clone())).await {
            Ok(Ok(response)) => Ok(response.code().to_string()),
            Ok(Err(err)) => {
                let code = err
                    .status()
                    .map_or_else(|| "no reply".into(), |code| code.to_string());
                let kind = if err.is_permanent() {
                    "refused"
                } else if err.is_transient() {
                    "deferred"
                } else {
                    "connection failed"
                };
                Err(format!("{kind}, {code}"))
            }
            Err(_) => Err("timed out".into()),
        }
    }
}

/// `message` as the library builds it, from `out`'s checked fields, its
/// ID at `domain`; none when the library refuses an address.
pub(crate) fn message(out: &Outgoing, id: &str) -> Option<Message> {
    let from: Address = out.from.parse().ok()?;
    let name = Some(out.from_name.clone()).filter(|name| !name.is_empty());
    let mut builder = Message::builder()
        .from(Mailbox::new(name, from))
        .subject(out.subject.clone())
        .message_id(Some(id.to_string()));
    for to in &out.to {
        builder = builder.to(mailbox(to)?);
    }
    for reply_to in &out.reply_to {
        builder = builder.reply_to(mailbox(reply_to)?);
    }
    let kind = if out.html {
        ContentType::TEXT_HTML
    } else {
        ContentType::TEXT_PLAIN
    };
    let part = SinglePart::builder()
        .header(kind)
        .header(ContentTransferEncoding::QuotedPrintable)
        .body(out.body.clone());
    builder.singlepart(part).ok()
}

/// `recipient` through the library's address parser.
fn mailbox(recipient: &Recipient) -> Option<Mailbox> {
    let address: Address = recipient.address.parse().ok()?;
    let name = Some(recipient.name.clone()).filter(|name| !name.is_empty());
    Some(Mailbox::new(name, address))
}
