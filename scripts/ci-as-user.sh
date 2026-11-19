#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Runs a command as an ordinary user inside a CI job container:
#   scripts/ci-as-user.sh <command> [args ...]
# The job containers run as root, which bypasses file permissions and which
# the server refuses to run as, while the tests expect an ordinary user.
# Needs the folders it writes to (target/, TMPDIR, the cargo and sccache
# caches) to be writable by that user, as scripts/ci-as-user.sh prepare
# makes them.
set -euo pipefail

uid=${CI_UID:-1000}
home=${CI_HOME:-/tmp/ci-home}

if [ "${1:-}" = prepare ]; then
  mkdir -p "$home" target "$TMPDIR"
  chown "$uid:$uid" "$home"
  chown -R "$uid:$uid" target
  chmod 1777 "$TMPDIR"
  # The caches belong to the job's own volumes; the user adds to them.
  for cache in "${CARGO_HOME:-}" "${SCCACHE_DIR:-}"; do
    [ -z "$cache" ] || chmod -R a+rwX "$cache"
  done
  exit 0
fi

exec setpriv --reuid="$uid" --regid="$uid" --clear-groups env HOME="$home" "$@"
