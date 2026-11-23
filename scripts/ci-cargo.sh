#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# cargo as the ordinary CI user (see ci-as-user.sh), for scripts that take
# the command to run in $CARGO, such as reference/switch-back/run.sh.
# With CI_USER_DOCKER_HOST set (see ci-docker-proxy.sh) the user's tests
# reach Docker through that socket, as the switch-back suite's SQL steps do.
set -euo pipefail
export DOCKER_HOST="${CI_USER_DOCKER_HOST:-${DOCKER_HOST:-}}"
exec "$(dirname "$0")/ci-as-user.sh" cargo "$@"
