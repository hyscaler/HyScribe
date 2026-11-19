#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# cargo as the ordinary CI user (see ci-as-user.sh), for scripts that take
# the command to run in $CARGO, such as reference/switch-back/run.sh.
set -euo pipefail
exec "$(dirname "$0")/ci-as-user.sh" cargo "$@"
