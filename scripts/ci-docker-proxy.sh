#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Lets the ordinary CI user of a job container use the runner's Docker:
#   scripts/ci-docker-proxy.sh [socket]       (default /tmp/ci-docker.sock)
# The runner's socket answers only to the job's root, so a root socat
# relays a world accessible socket to it. Export CI_USER_DOCKER_HOST with
# the printed unix:// address for scripts/ci-cargo.sh. Runs in the job
# container, which is thrown away with its relay.
set -euo pipefail

sock=${1:-/tmp/ci-docker.sock}
rm -f "$sock"
setsid nohup socat "UNIX-LISTEN:$sock,fork,mode=666" "UNIX-CONNECT:${DOCKER_HOST#unix://}" >/dev/null 2>&1 &
for _ in 1 2 3 4 5 6 7 8 9 10; do
  [ -S "$sock" ] && break
  sleep 0.5
done
test -S "$sock"
echo "CI_USER_DOCKER_HOST=unix://$sock" >> "${GITHUB_ENV:-/dev/null}"
echo "unix://$sock"
