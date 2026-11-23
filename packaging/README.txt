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

Verify this download
--------------------

  sha256sum -c SHA256SUMS.txt --ignore-missing
  gh attestation verify hyscribe_<version>_linux_<arch>.tar.gz --repo hyscaler/HyScribe

License: AGPL-3.0-or-later (see LICENSE). Source: https://github.com/hyscaler/HyScribe
Security reports: security@hyscribe.com
