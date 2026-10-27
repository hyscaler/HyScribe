#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Installs a release archive the way DEPLOYMENT.md describes and checks the
# service commands: scripts/smoke-test.sh <archive>
set -euo pipefail

archive="$(realpath "${1:?archive}")"
work="$(mktemp -d)"
trap 'cd /; "$work/hyscribe/hyscribe" service stop >/dev/null 2>&1 || true; rm -rf "$work"' EXIT
cd "$work"

tar -xzf "$archive"
cd hyscribe
test -x hyscribe && test -f LICENSE && test -f README.txt && test -f .env.example

./hyscribe version | grep -q 'AGPL-3.0-or-later'
printf 'DB_NAME=wordpress\nDB_USER=wp\n' > .env
chmod 600 .env

./hyscribe service start | grep -q '^started (pid '
./hyscribe service status | grep -q '^running (pid '
./hyscribe service stop | grep -q '^stopped (pid '
status=0
./hyscribe service status > status.txt || status=$?
test "$status" -eq 3
grep -q 'stopped on signal 15' status.txt
echo "smoke test passed: $(basename "$archive")"
