# 0016: The mail transport

- Status: Accepted (2026-11-16)
- Date: 2026-11-16

## Context

WordPress sends mail through `wp_mail()`, which hands each message to PHPMailer and, on most hosts, PHP's `mail()` and the host's local `sendmail`. HyScribe ships as one binary with no runtime dependency but the database (ADR 0005), so it cannot count on a local mail agent; it has to speak SMTP to a relay the owner names.

The messages themselves are ported in `hyscribe-mail` (MVP 8.2): each of WordPress's mail functions built as a pure function returning what it hands `wp_mail()` (recipient, subject, body and headers), matched byte for byte with PHP by `reference/mail/golden.sh`. What is left is the transport: how a message leaves the server, and what happens when it cannot.

What PHP WordPress does today:

- `wp_mail()` returns `false` when PHPMailer fails, and most callers carry on: comment notifications, new user notices and the password and email change notices ignore the result. A few report it: `retrieve_password()` answers `retrieve_password_email_failure` ("The email could not be sent..."), the personal data requests answer `privacy_email_error`, and `_wp_privacy_send_request_confirmation_notification()` only marks the request notified when sending worked
- The default sender is `WordPress <wordpress@example.com>`, the domain being the site's host without `www.`; comment notifications set the commenter's name as the sender's name
- Nothing limits how many messages are sent: anyone can ask for password reset mails for any account, as often as they like
- Sending is synchronous: the request waits for the SMTP conversation

## Decision

Accepted on 2026-11-16 with amendments: messages over the overall rate limit wait in a bounded queue instead of being dropped, all mail goes through one `wp_mail()` equivalent with WordPress's filters, and a hook point stands in for `phpmailer_init`.

### Settings in `.env`

| Key | Meaning | Default |
| --- | --- | --- |
| `SMTP_HOST` | The relay's host name; unset means no transport | unset |
| `SMTP_PORT` | The relay's port | 587 for `starttls`, 465 for `tls`, 25 for `none` |
| `SMTP_TLS` | `starttls` (STARTTLS required), `tls` (TLS from the first byte) or `none` | `starttls` |
| `SMTP_USER` | The login for SMTP AUTH; unset means no AUTH | unset |
| `SMTP_PASSWORD` | Its password | unset |
| `MAIL_FROM` | The sender address of every message | WordPress's default sender: `wordpress@` and the site's host without `www.` |

- `SMTP_TLS=none` is accepted only for a loopback host (a relay on the same machine), and never with `SMTP_USER`: credentials are never sent unencrypted
- There is no opportunistic mode: with `starttls`, a server that does not offer STARTTLS fails the send rather than falling back to plain text
- Real environment variables win over `.env`, as for every key (docs/DEPLOYMENT.md); `.env.example` documents the keys, and the setup wizard (MVP 8.4) can ask for them
- The EHLO name is the site's host, so nothing reads the machine's host name

### TLS

- Through `rustls`, with the host's certificate store (`rustls-native-certs`), as decided for outbound fetches; the crypto provider is the one the HTTP client's ADR picks, so the binary carries one
- A host without a certificate store sends no mail over TLS and logs why, as the outbound fetches do
- The relay's certificate must match `SMTP_HOST`; there is no setting to skip verification

### No transport configured

- With `SMTP_HOST` unset, sending returns `false` at once, as `wp_mail()` does when PHPMailer fails, and each caller answers as PHP answers then: the lost password form shows `retrieve_password_email_failure`, a privacy request shows `privacy_email_error`, and every other caller carries on
- Each message not sent is logged at warn level with its kind; the service logs once at start that mail is off
- In shadow mode nothing writes, so nothing sends

### When a message is sent

- After the action's transaction commits, so a mail never announces a change that rolled back; PHP sends mid-request, so only the moment the mail leaves differs
- Synchronously where the answer depends on the result (password resets, privacy requests), with a 30 second limit on the whole SMTP conversation; in the background where PHP ignores the result (comment notifications, new user and change notices), so a commenter does not wait for the relay
- No retries: a failed send is logged and dropped, as in PHP. The only queue is the overall rate limit's (below)

### Rate limits

WordPress has none; HyScribe adds them because its mail goes through the owner's relay account, whose reputation and quota a flood would spend:

- At most 3 password reset mails per account per hour, and 10 per client address per hour (the address as the comment flood check takes it). A reset mail over these is not sent and counts as a failed send, with `retrieve_password()`'s PHP answer for one; it is logged with the limit it hit
- At most 60 messages a minute in all. A message over it is not dropped: it waits in a bounded queue and leaves when the minute allows. Only when the queue is full is a message refused, counted as a failed send and logged
- A caller that waits for the result (a password reset, a privacy request) waits within the 30 second limit; a queued message that cannot leave in time is a failed send

### Header injection

The ported messages carry what users typed: a commenter's name goes into `From:`, an email address into `To:` and `Reply-To:`, a post title into the subject. Saving already strips line breaks from most of these, but the transport does not rely on it:

- The headers string is parsed as `wp_mail()` parses it (one header per line, `From`, `Content-Type`, `Reply-To`, `Cc` and `Bcc` understood); only `From` (its name), `Reply-To` and `Content-Type`'s charset are used, other headers are dropped and logged
- A CR, LF or NUL in the recipient, the subject, a header's value or the sender's name refuses the message (a failed send, logged)
- Addresses go through the SMTP library's address parser; one it refuses refuses the message
- The sender address is always `MAIL_FROM`; a `From` header only sets the name shown, so a relay that checks senders accepts every message, and no message can claim another domain. This differs from PHP only when `MAIL_FROM` is set
- The subject and names are encoded (RFC 2047) and the body sent as `text/plain; charset=UTF-8` with CRLF line ends and dot stuffing, by the library

### What is logged

- One line per message: the message kind (for example `retrieve_password`), the recipient's domain only, the outcome, the relay's reply code and the time taken
- Never a message body, a subject, a full address, a password reset key, a confirmation key or hash, an export link, or the SMTP password
- Relay errors are logged by their reply code and the library's error kind, not the server's full text, which can echo the message

### One `wp_mail()`

- Every message goes through one function equivalent to `wp_mail()`, which runs WordPress's filters and actions under their names, with WordPress's arguments, in WordPress's order: `pre_wp_mail` (a value other than null ends the call with it), `wp_mail` (the recipient, subject, message, headers and attachments), `wp_mail_from`, `wp_mail_from_name`, `wp_mail_content_type`, `wp_mail_charset`, then `wp_mail_succeeded` or `wp_mail_failed`
- A hook point standing in for `phpmailer_init` lets an extension replace the transport (an API based sender, for example). It is given the message after the filters, not PHPMailer's object
- Until the extension runtime exists these are plain internal calls (docs/EXTENSIONS.md). A filter's result goes through the same header injection checks and the same `MAIL_FROM` rule as the message it replaced, so no hook skips a check

### The library: `lettre`

| | `lettre` 0.11 | `mail-send` 0.7 | `async-smtp` 0.10 | Our own client |
| --- | --- | --- | --- | --- |
| Licence | MIT | Apache-2.0 OR MIT | MIT OR Apache-2.0 | AGPL-3.0-or-later |
| `unsafe` | `#![forbid(unsafe_code)]` | none in the crate, not forbidden | 2 blocks (`Pin::new_unchecked`) | none |
| FFI | none with the features below (the `hostname` and `native-tls` features, which have it, stay off) | none in the crate | none | none |
| TLS | `tokio1-rustls`, with `rustls-native-certs`, `ring` or `aws-lc-rs` | `tokio-rustls` with `rustls-platform-verifier`, `aws-lc-rs` by default | none: the caller wraps the stream | `tokio-rustls` |
| Maintenance | active, release 2026-08, about 9 million downloads in 90 days | active (Stalwart), release 2026-11 | release 2025-05 | ours |
| Size | about 17,000 lines, 7,000 of them SMTP | about 2,300 lines, plus `mail-builder`, `mail-auth`, `smtp-proto` | about 2,500 lines | a few hundred lines and a fuzzed reply parser |
| cargo vet | no audit in the imported sets for `lettre`, `email-encoding`, `email_address` or `quoted_printable` | none for `mail-send`, `mail-builder`, `smtp-proto` or `mail-auth` | none | nothing to import |

The TLS crates themselves are covered: `rustls`, `tokio-rustls`, `rustls-webpki` and `ring` are trusted by Mozilla's set and audited by Bytecode Alliance's or Google's; `rustls-native-certs` has no audit, `openssl-probe` (which it uses on Linux) one from Bytecode Alliance. `nom`, `mime`, `base64`, `idna`, `url`, `socket2`, `futures-io` and `async-trait` are audited in the imported sets; several are already in HyScribe's tree.

Decided: `lettre`, with `default-features = false` and `smtp-transport`, `tokio1`, `tokio1-rustls`, `rustls-native-certs` and `builder`, plus the provider feature the HTTP client's ADR picks. It forbids `unsafe`, is the most used and maintained of the three, builds the MIME message (encoding, folding, dot stuffing) that a client of our own would have to get right too, and its TLS modes map onto `SMTP_TLS` directly. The cost is the audit: HyScribe reviews and certifies `lettre` (its SMTP and builder modules), `email-encoding`, `email_address`, `quoted_printable` and `rustls-native-certs` itself with cargo vet, in `supply-chain/audits.toml`, since no imported set covers them. `mail-send` pulls DKIM and a different certificate verifier than the one decided; `async-smtp` has no TLS and is the least maintained. Our own client is the fallback if the audit finds something we would not ship.

### Shape

- The transport lives in `hyscribe-mail` beside the messages, behind the one `wp_mail()` equivalent, an async `send(&Mail, kind) -> bool`, so the save handlers call one function and tests swap in a recorder
- `hyscribe-mail` keeps the messages pure; only the transport module depends on `lettre`

### The commenter's host

PHP's comment notifications name the commenter's host from `gethostbyaddr()`; Rust's standard library has no reverse lookup. The notifications show the commenter's IP address in its place, which is what PHP itself shows when the lookup fails. A reverse lookup comes later, with the HTTP client work.

## Decisions (2026-11-16)

1. The settings and their names are accepted; `MAIL_FROM` defaults to WordPress's default sender, `wordpress@` and the site's host without `www.`
2. `lettre` is the library; HyScribe reviews and certifies it and its uncovered dependencies with cargo vet
3. Rate limits: the per account password reset limit stays; over the overall limit, messages wait in a bounded queue instead of being dropped
4. Messages whose result PHP ignores are sent in the background
5. Comment notifications show the commenter's IP address, PHP's own fallback; a reverse lookup comes with the HTTP client work
6. All mail goes through one `wp_mail()` equivalent running WordPress's filters in order, and a hook point standing in for `phpmailer_init` lets an extension replace the transport

## Consequences

- The save handlers that answer today as if mail succeeded (password reset links, profile email changes, the admin email change; docs/SECURITY_MODEL.md) call the transport once this is accepted; `hyscribe-mail`'s crate documentation lists where each message is sent from
- The lost password and reset flows (MVP 8.3) depend on it
- A site without `SMTP_HOST` works, but users cannot reset their own passwords, as on a PHP host whose mail is broken; the wizard and the service's start log say so
- New dependencies and their audits enter `supply-chain/`; `cargo deny` already allows their licences
- An SMTP reply parser is the library's, so it is covered by the library's tests and the audit rather than a fuzz target of ours; a client of our own would need one
