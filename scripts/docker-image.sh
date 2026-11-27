#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the container image of docker/Dockerfile from release archives
# (scripts/package.sh's hyscribe_<version>_linux_<arch>.tar.gz), never from
# source (ADR 0005, amendment of 2026-11-27):
#   scripts/docker-image.sh <dist> load <name:tag>        amd64 only, into the local store
#   scripts/docker-image.sh <dist> oci <file.tar>         amd64 and arm64, an OCI archive
#   scripts/docker-image.sh <dist> push <name:tag> ...    amd64 and arm64, pushed under each name
# The build runs nothing (no RUN), so arm64 needs no emulation. It is
# reproducible: the base is pinned by digest, the files and timestamps come
# from the archives and SOURCE_DATE_EPOCH (the commit's time by default),
# and BuildKit's own provenance, which records the time of the build, is
# left out (the release attests the image instead). `oci` then checks each
# platform's binary is its archive's, byte for byte, and prints the image's
# digest, which `push` prints too. Needs docker with buildx and jq, and an
# image store that keeps multi-platform images (Docker's containerd store)
# for `oci` and `push`. HYSCRIBE_IMAGE_NO_CACHE=1 builds without BuildKit's
# cache, for a check that a rebuild gives the same bytes.
set -euo pipefail

dist=${1:?dist folder}
how=${2:?load, oci or push}
shift 2
[ $# -gt 0 ] || { echo "usage: $0 <dist> load|oci|push <target> ..." >&2; exit 2; }
DOCKER="${DOCKER:-docker}"
root="$(cd "$(dirname "$0")/.." && pwd)"

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}"
revision="$(git -C "$root" rev-parse HEAD)"
created="$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ)"

case "$how" in
  load) platforms=linux/amd64 ;;
  oci | push) platforms=linux/amd64,linux/arm64 ;;
  *) echo "unknown output: $how" >&2; exit 2 ;;
esac

# The build context: the archives and nothing else.
context="$(mktemp -d)"
trap 'rm -rf "$context"' EXIT
for platform in ${platforms//,/ }; do
  archive="$dist/hyscribe_${version}_linux_${platform#linux/}.tar.gz"
  test -f "$archive" || { echo "no $archive" >&2; exit 1; }
  cp "$archive" "$context/"
done

image=rewrite-timestamp=true
case "$how" in
  load) output=(--output "type=docker,name=$1,$image") ;;
  oci) output=(--output "type=oci,dest=$1,$image") ;;
  push)
    names="$(IFS=,; echo "$*")"
    output=(--output "type=image,\"name=$names\",push=true,unpack=false,oci-mediatypes=true,$image"
      --metadata-file "$context/metadata.json")
    ;;
esac

"$DOCKER" buildx build --file "$root/docker/Dockerfile" --platform "$platforms" \
  --provenance=false --sbom=false ${HYSCRIBE_IMAGE_NO_CACHE:+--no-cache} \
  --build-arg "VERSION=$version" --build-arg "REVISION=$revision" --build-arg "CREATED=$created" \
  "${output[@]}" "$context"

if [ "$how" = push ]; then
  sed -n 's/.*"containerimage.digest": *"\(sha256:[0-9a-f]*\)".*/\1/p' "$context/metadata.json"
fi

# The OCI archive's binary for each platform is the archive's, byte for
# byte; then its digest, the one `push` prints for the same inputs.
if [ "$how" = oci ]; then
  oci="$context/oci"
  mkdir "$oci"
  tar -xf "$1" -C "$oci"
  blob() { echo "$oci/blobs/sha256/${1#sha256:}"; }
  index="$(jq -r '.manifests[0].digest' "$oci/index.json")"
  for platform in ${platforms//,/ }; do
    arch=${platform#linux/}
    manifest="$(jq -r --arg arch "$arch" '.manifests[] | select(.platform.architecture == $arch) | .digest' "$(blob "$index")")"
    want="$(tar -xzOf "$context/hyscribe_${version}_linux_$arch.tar.gz" hyscribe/hyscribe | sha256sum | cut -d' ' -f1)"
    found=
    for layer in $(jq -r '.layers[].digest' "$(blob "$manifest")"); do
      if tar -tzf "$(blob "$layer")" usr/local/bin/hyscribe > /dev/null 2>&1; then
        found="$(tar -xzOf "$(blob "$layer")" usr/local/bin/hyscribe | sha256sum | cut -d' ' -f1)"
      fi
    done
    test "$found" = "$want" || { echo "$platform: the image's binary is not the archive's" >&2; exit 1; }
    echo "$platform: binary sha256:$want, the archive's"
  done
  echo "$index"
fi
