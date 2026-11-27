#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Installs a release archive the way DEPLOYMENT.md describes, points it at
# the default install fixture (reference/default-install/db.sh up) and checks
# the service commands and a page: scripts/smoke-test.sh <archive>
# On a machine without Docker, reference/default-install/db-native.sh up
# gives the same fixture. Runs on Linux and macOS, and on Windows in Git
# for Windows's bash, where the archive is a zip that PowerShell unpacks as
# a user would.
set -euo pipefail

archive="$(cd "$(dirname "${1:?archive}")" && pwd)/$(basename "$1")"
work="$(mktemp -d)"
trap 'cd /; "$work/hyscribe/hyscribe" service stop >/dev/null 2>&1 || true; rm -rf "$work"' EXIT
cd "$work"

case "$archive" in
  *.zip)
    powershell -NoProfile -Command "Expand-Archive -LiteralPath '$(cygpath -w "$archive")' -DestinationPath '$(cygpath -w "$work")'"
    ;;
  *) tar -xzf "$archive" ;;
esac
cd hyscribe
# Git for Windows's bash finds hyscribe.exe as hyscribe.
test -x hyscribe && test -f LICENSE && test -f NOTICE && test -f README.txt && test -f .env.example
grep -q '^Part 1: WordPress and its themes$' THIRD-PARTY-NOTICES
grep -q '^Part 2: Rust crates$' THIRD-PARTY-NOTICES
for page in README install configuration service switching maintenance security not-supported docker; do
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
