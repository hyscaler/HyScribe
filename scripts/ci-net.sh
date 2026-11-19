#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Lets a CI job container use the fixture containers that db.sh and php.sh
# start through the host's Docker (reference/default-install):
#   scripts/ci-net.sh up <network> <port>=<container>:<container-port> ...
#   scripts/ci-net.sh down <network>
# The job container joins the fixtures' network and forwards each local
# port to its container, because a published port is not reachable from
# inside a container. Runs in the job container (its hostname is its id).
set -euo pipefail

DOCKER="${DOCKER:-docker}"
action=${1:?up or down}
network=${2:?network}
shift 2

case "$action" in
  up)
    "$DOCKER" network inspect "$network" >/dev/null 2>&1 || "$DOCKER" network create "$network" >/dev/null
    "$DOCKER" network connect "$network" "$(hostname)" 2>/dev/null || true
    for map in "$@"; do
      port=${map%%=*}
      target=${map#*=}
      setsid nohup socat "TCP-LISTEN:$port,bind=127.0.0.1,fork,reuseaddr" "TCP:$target" >/dev/null 2>&1 &
    done
    ;;
  down)
    "$DOCKER" network disconnect -f "$network" "$(hostname)" >/dev/null 2>&1 || true
    "$DOCKER" network rm "$network" >/dev/null 2>&1 || true
    ;;
  *)
    echo "usage: $0 up|down <network> [<port>=<container>:<port> ...]" >&2
    exit 2
    ;;
esac
