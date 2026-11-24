#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Installs a release archive the way DEPLOYMENT.md describes, points it at
# the default install fixture (reference/default-install/db.sh up) and checks
# the service commands and a page: scripts/smoke-test.sh <archive>
set -euo pipefail

archive="$(realpath "${1:?archive}")"
work="$(mktemp -d)"
trap 'cd /; "$work/hyscribe/hyscribe" service stop >/dev/null 2>&1 || true; rm -rf "$work"' EXIT
cd "$work"

tar -xzf "$archive"
cd hyscribe
test -x hyscribe && test -f LICENSE && test -f NOTICE && test -f README.txt && test -f .env.example
grep -q '^Part 1: WordPress and its themes$' THIRD-PARTY-NOTICES
grep -q '^Part 2: Rust crates$' THIRD-PARTY-NOTICES
for page in README install configuration service switching maintenance security not-supported; do
  test -s "docs/$page.md"
done

./hyscribe version | grep -q 'AGPL-3.0-or-later'
port="${SMOKE_PORT:-18080}"
printf 'DB_HOST=127.0.0.1\nDB_PORT=%s\nDB_NAME=wordpress\nDB_USER=wordpress\nDB_PASSWORD=reference-only\nPORT=%s\n' \
  "${HYSCRIBE_DB_PORT:-33061}" "$port" > .env
chmod 600 .env

./hyscribe service start | grep -q '^started (pid '
./hyscribe service status | grep -q '^running (pid '
# Saved first: grep -q stops reading early, which fails curl under pipefail.
curl -fsS "http://127.0.0.1:$port/?p=1" > page.html
grep -q 'Hello world!' page.html
./hyscribe service stop | grep -q '^stopped (pid '
status=0
./hyscribe service status > status.txt || status=$?
test "$status" -eq 3
grep -q 'stopped on signal 15' status.txt
echo "smoke test passed: $(basename "$archive")"
