#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Runs the container image the way docs/operators/docker.md describes,
# against the default install fixture (reference/default-install/db.sh up,
# whose database answers as "db" on HYSCRIBE_REF_NETWORK):
#   scripts/docker-smoke-test.sh <image>
# First the existing site: the fixture's database from the environment, a
# page, the health check, a clean stop. Then a new site: setup mode on an
# empty volume, the browser wizard answered with curl into the empty
# wordpress_saves database, the home page, and the site again after a
# restart from what the volume kept. Both run as the image's own user, on a
# read only root filesystem, with the installation on a named volume. The
# image comes from scripts/docker-image.sh <version> load <image>.
# The pages are fetched through a published port on 127.0.0.1, or, with
# SMOKE_VIA_NETWORK=1 (a CI job container on the same network), from the
# container by name. Containers and volumes are named after SMOKE_NAME and
# removed at the end, however it ends.
set -euo pipefail

image=${1:?image}
DOCKER="${DOCKER:-docker}"
network="${HYSCRIBE_REF_NETWORK:-hyscribe-ref}"
name="${SMOKE_NAME:-hs-image-smoke-$$}"
port="${SMOKE_PORT:-18081}"
work="$(mktemp -d)"
cleanup() {
  "$DOCKER" rm -fv "$name-site" "$name-new" >/dev/null 2>&1 || true
  "$DOCKER" volume rm "$name-site" "$name-new" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

# Runs the image detached as <container>, with the installation on the
# volume of the same name and the remaining arguments as options.
run() {
  local container=$1
  shift
  local reach=(-p "127.0.0.1:$port:8080")
  [ "${SMOKE_VIA_NETWORK:-}" = 1 ] && reach=()
  "$DOCKER" run -d --name "$container" --network "$network" --read-only \
    --health-interval 2s --health-start-period 30s \
    -v "$container:/hyscribe" "${reach[@]}" "$@" "$image" >/dev/null
}
base() {
  if [ "${SMOKE_VIA_NETWORK:-}" = 1 ]; then echo "http://$1:8080"; else echo "http://127.0.0.1:$port"; fi
}
# Waits up to a minute for a line in the container's log.
logged() {
  for _ in $(seq 60); do
    "$DOCKER" logs "$1" 2>&1 | grep -q "$2" && return 0
    sleep 1
  done
  echo "$1 did not log: $2" >&2
  "$DOCKER" logs --tail 30 "$1" >&2
  return 1
}
healthy() {
  for _ in $(seq 60); do
    [ "$("$DOCKER" inspect -f '{{.State.Health.Status}}' "$1")" = healthy ] && return 0
    sleep 1
  done
  echo "$1 is not healthy" >&2
  return 1
}
# Stops the container with SIGTERM, as `docker stop` does, and checks it
# stopped by itself and exited with 0.
stopped() {
  "$DOCKER" stop -t 30 "$1" >/dev/null
  test "$("$DOCKER" inspect -f '{{.State.ExitCode}} {{.State.OOMKilled}}' "$1")" = "0 false"
  "$DOCKER" logs "$1" 2>&1 | grep -q 'stopped on signal 15'
}

"$DOCKER" run --rm "$image" version | grep -q 'AGPL-3.0-or-later'
# The image's user, which a container runs as, and the CA bundle the binary reads.
test "$("$DOCKER" image inspect -f '{{.Config.User}}' "$image")" = 65532:65532
test "$("$DOCKER" run --rm --entrypoint id "$image")" = 'uid=65532(nonroot) gid=65532(nonroot) groups=65532(nonroot)'
test "$("$DOCKER" run --rm --entrypoint grep "$image" -c 'BEGIN CERTIFICATE' /etc/ssl/certs/ca-certificates.crt)" -gt 100

# The existing site, configured by the environment alone.
site="$name-site"
run "$site" -e DB_HOST=db -e DB_NAME=wordpress -e DB_USER=wordpress -e DB_PASSWORD=reference-only
logged "$site" 'in shadow (read only) mode, listening on http://0.0.0.0:8080'
# Saved first: grep -q stops reading early, which fails curl under pipefail.
curl -fsS "$(base "$site")/?p=1" > "$work/page.html"
grep -q 'Hello world!' "$work/page.html"
healthy "$site"
"$DOCKER" exec "$site" hyscribe service status | grep -q '^running (pid 1)$'
stopped "$site"
"$DOCKER" rm -v "$site" >/dev/null

# A new site through the browser wizard, as a person would answer it.
new="$name-new"
run "$new"
logged "$new" 'No .env: setup mode'
url="$(base "$new")"
# The secret is only in the volume; `docker cp` reads it as the docs say.
"$DOCKER" cp "$new:/hyscribe/logs/setup-secret" - | tar -xO > "$work/secret"
secret="$(sed 's/.* //' "$work/secret")"
test "${#secret}" -eq 32
jar="$work/cookies"
token() { sed -n 's/.*name="_csrf" value="\([^"]*\)".*/\1/p' "$1" | head -n 1; }
curl -fsS -c "$jar" "$url/installation/step-1" > "$work/step1.html"
curl -fsS -b "$jar" -c "$jar" -o /dev/null -w '%{http_code}\n' \
  --data-urlencode "_csrf=$(token "$work/step1.html")" --data-urlencode "secret=$secret" \
  "$url/installation/step-1" | grep -qx 303
curl -fsS -b "$jar" "$url/installation/step-2" > "$work/step2.html"
csrf="$(token "$work/step2.html")"
curl -fsS -b "$jar" -o /dev/null -w '%{redirect_url}\n' \
  --data-urlencode "_csrf=$csrf" --data-urlencode new=1 \
  "$url/installation/step-2" | grep -q '/installation/step-3$'
curl -fsS -b "$jar" "$url/installation/step-3" | grep -q 'name="weblog_title"'
curl -fsS -b "$jar" "$url/installation/step-3" \
  --data-urlencode "_csrf=$csrf" --data-urlencode DB_NAME=wordpress_saves \
  --data-urlencode DB_USER=wordpress --data-urlencode DB_PASSWORD=reference-only \
  --data-urlencode DB_HOST=db --data-urlencode DB_PORT=3306 \
  --data-urlencode 'weblog_title=A Docker Site' --data-urlencode user_name=owner \
  --data-urlencode 'admin_password=correct horse battery' \
  --data-urlencode 'admin_password2=correct horse battery' \
  --data-urlencode admin_email=owner@example.org --data-urlencode "site_address=$url" \
  > "$work/step3.html"
grep -q '<h1>HyScribe is set up</h1>' "$work/step3.html" || { cat "$work/step3.html" >&2; exit 1; }
logged "$new" 'in primary mode, listening on http://0.0.0.0:8080'
curl -fsS "$url/" > "$work/home.html"
grep -q 'A Docker Site' "$work/home.html"
grep -q 'Hello world!' "$work/home.html"
# The volume keeps .env: the site comes back after a restart.
"$DOCKER" restart -t 30 "$new" >/dev/null
for _ in $(seq 30); do
  [ "$("$DOCKER" logs "$new" 2>&1 | grep -c 'in primary mode, listening')" -ge 2 ] && break
  sleep 1
done
curl -fsS "$url/" > "$work/home.html"
grep -q 'A Docker Site' "$work/home.html"
healthy "$new"
stopped "$new"
echo "image smoke test passed: $image"
