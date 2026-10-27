HyScribe
========

A Rust drop-in replacement for WordPress. This folder is the whole
installation: everything HyScribe writes stays inside it.

Start
-----

  cp .env.example .env && chmod 600 .env    then fill in the database
  ./hyscribe service start
  ./hyscribe service status
  ./hyscribe service log -f
  ./hyscribe service stop

HyScribe refuses to run as root. By default it listens on 127.0.0.1:8080
and runs in shadow mode (read only).

Verify this download
--------------------

  sha256sum -c SHA256SUMS.txt --ignore-missing
  gh attestation verify hyscribe_<version>_linux_<arch>.tar.gz --repo hyscaler/HyScribe

License: AGPL-3.0-or-later (see LICENSE). Source: https://github.com/hyscaler/HyScribe
Security reports: security@hyscribe.com
