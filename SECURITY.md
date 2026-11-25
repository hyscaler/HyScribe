# Security Policy

Security is HyScribe's first priority. When security conflicts with compatibility, performance or convenience, security wins. The design behind this is in [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md).

## Reporting a vulnerability

Please report security vulnerabilities privately. Do not open a public issue or pull request for a security problem.

- Preferred: open a private security advisory through the repository's Security tab (GitHub Security Advisories), or
- Email: security@hyscribe.com

Include a description of the issue, steps to reproduce, the affected component (core, a crate, a plugin or theme port, the wizard, the admin), the HyScribe version, and any proof of concept.

Targets:

| Step | Target |
| --- | --- |
| Acknowledge report | 2 business days |
| Triage and severity assessment | 7 days |
| Fix for critical and high severity | As fast as possible, aiming for 14 days |
| Coordinated public disclosure | After a fix ships, or 90 days from the report, whichever comes first |

Please give us a reasonable opportunity to fix the issue before any public disclosure. We credit reporters in the advisory unless asked not to.

## Scope

In scope: the `hyscribe` binary, all crates in this repository, first-party plugin and theme ports, the extension sandbox, release artifacts and the update path.

Also in scope: a port that reproduces a known vulnerability of the WordPress plugin it replaces.

## Supported versions

HyScribe's first release is v0.1.0. Security fixes target the latest release and the `development` branch. A longer support policy will be published with 1.0.

## Deployment hardening

- Run HyScribe as an unprivileged user; the binary refuses to run as root
- Keep `.env` readable only by that user (the binary refuses to start if it is world readable)
- Serve over HTTPS behind a reverse proxy with `BEHIND_PROXY=true`: HyScribe itself serves plain HTTP
- Verify downloads against the release's `SHA256SUMS.txt` and its build provenance (`gh attestation verify <file> --repo hyscaler/HyScribe`)
- Leave legacy attack-prone endpoints (XML-RPC, pingbacks) disabled unless you need them
