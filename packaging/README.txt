HyScribe
========

A Rust drop-in replacement for WordPress. This folder is the whole
installation: everything HyScribe writes stays inside it.

Start
-----

  ./hyscribe service start      without .env, starts the setup wizard and
                                prints its address and one-time secret
                                (or puts them in logs/setup-secret)
  ./hyscribe service status
  ./hyscribe service log -f
  ./hyscribe service stop

The wizard reads an existing site's wp-config.php and writes .env. It
also runs in the terminal (./hyscribe setup), or copy .env.example to
.env, chmod 600 it and fill in the database.

The docs folder explains installing, configuration, the service,
switching a site over and back, backups, upgrades and security; start
with docs/README.md.

HyScribe refuses to run as root. By default it listens on 127.0.0.1:8080
and runs in shadow mode (read only).

The same binary also comes as a container image, docker.io/hyscribe/hyscribe
(or ghcr.io/hyscaler/hyscribe): docs/docker.md.

On Windows the binary is hyscribe.exe: run .\hyscribe.exe service start
from this folder. Keep the folder inside your own profile folder, as
HyScribe sets no file permissions there: docs/install.md, "On Windows".

Verify this download
--------------------

  sha256sum -c SHA256SUMS.txt --ignore-missing
  gh attestation verify hyscribe_<version>_<os>_<arch>.tar.gz --repo hyscaler/HyScribe

On macOS, shasum -a 256 -c does what sha256sum -c does. The macOS binary
is not signed or notarised by Apple yet: docs/install.md, "On macOS", says
what that means for a downloaded copy. On Windows, compare the output of
certutil -hashfile <zip> SHA256 with SHA256SUMS.txt; the binary is not
signed yet either, which docs/install.md, "On Windows", explains.

License: AGPL-3.0-or-later (see LICENSE and NOTICE). Source: https://github.com/hyscaler/HyScribe
HyScribe is developed and supported by HyScaler (https://hyscaler.com).
The binary also embeds WordPress's files, its themes and Rust crates under
their own licences: see THIRD-PARTY-NOTICES, which also says where their
source is.
Security reports: security@hyscribe.com
