#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Prepares a CI job container (the pinned Rust image of .github/workflows):
#   scripts/ci-setup.sh [cargo-tool@version ...]
# Installs the system packages the gates need and the named cargo tools into
# CARGO_HOME, which the runner keeps in a named cache volume, so a tool is
# compiled once and not again until its version changes.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
# The container has no DAC_OVERRIDE (ci.yml), so apt cannot hand its download
# folders to the _apt user and read them back; it downloads as root,
# in folders it owns.
echo 'APT::Sandbox::User "root";' > /etc/apt/apt.conf.d/99ci
chown root:root /var/lib/apt/lists/partial /var/cache/apt/archives/partial
apt-get update -qq
apt-get install -y -qq --no-install-recommends docker-cli socat clang jq >/dev/null

for spec in "$@"; do
  name=${spec%@*}
  version=${spec#*@}
  if ! cargo install --list | grep -qx "$name v$version:"; then
    # A warning-free build is the project's rule, not its dependencies'.
    env -u RUSTFLAGS -u RUSTC_WRAPPER cargo install --locked "$name" --version "$version"
  fi
done
echo "$CARGO_HOME/bin" >> "${GITHUB_PATH:-/dev/null}"
