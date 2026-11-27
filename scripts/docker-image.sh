#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the container image of docker/Dockerfile for a published release,
# from that release's Linux archives on GitHub, never from source (ADR 0005,
# amendment of 2026-11-27):
#   scripts/docker-image.sh <version> load <name:tag>        amd64 only, into the local store
#   scripts/docker-image.sh <version> oci <file.tar>         amd64 and arm64, an OCI archive
#   scripts/docker-image.sh <version> push <name:tag> ...    amd64 and arm64, pushed under each name
# <version> is a release tag such as v0.1.0. First each archive must carry
# the release workflow's build attestation for that tag and match its line
# in the release's SHA256SUMS.txt; the build then downloads it again and
# checks it against that SHA-256. It is reproducible while Debian's
# ca-certificates package stays the same: the bases are pinned by digest,
# the files and timestamps come from the archives and SOURCE_DATE_EPOCH (the
# release commit's time), and BuildKit's own provenance, which records the
# time of the build, is left out (the workflow attests the image instead).
# `oci` then checks each platform's binary is its archive's, byte for byte,
# prints each platform's compressed size and the image's digest, which
# `push` prints too. Needs docker with buildx, an image store that keeps
# multi-platform images (Docker's containerd store) for `oci` and `push`,
# jq and an authenticated gh. HYSCRIBE_IMAGE_NO_CACHE=1 builds without
# BuildKit's cache, for a check that a rebuild gives the same bytes.
set -euo pipefail

tag=${1:?version, such as v0.1.0}
how=${2:?load, oci or push}
shift 2
[ $# -gt 0 ] || { echo "usage: $0 <version> load|oci|push <target> ..." >&2; exit 2; }
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "not a release tag: $tag" >&2; exit 2; }
DOCKER="${DOCKER:-docker}"
repo=hyscaler/HyScribe
root="$(cd "$(dirname "$0")/.." && pwd)"
version=${tag#v}

case "$how" in
  load) platforms=linux/amd64 ;;
  oci | push) platforms=linux/amd64,linux/arm64 ;;
  *) echo "unknown output: $how" >&2; exit 2 ;;
esac

# The archives, outside the build context, which stays empty.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/release" "$work/context"
gh release download "$tag" --repo "$repo" --dir "$work/release" --pattern SHA256SUMS.txt
gh attestation verify "$work/release/SHA256SUMS.txt" --repo "$repo" \
  --signer-workflow "$repo/.github/workflows/release.yml" --source-ref "refs/tags/$tag" >/dev/null
sums=()
for platform in ${platforms//,/ }; do
  arch=${platform#linux/}
  file="hyscribe_${version}_linux_$arch.tar.gz"
  gh release download "$tag" --repo "$repo" --dir "$work/release" --pattern "$file"
  gh attestation verify "$work/release/$file" --repo "$repo" \
    --signer-workflow "$repo/.github/workflows/release.yml" --source-ref "refs/tags/$tag" >/dev/null
  line="$(grep -E "^[0-9a-f]{64}  $file\$" "$work/release/SHA256SUMS.txt")" || { echo "$file is not in SHA256SUMS.txt" >&2; exit 1; }
  (cd "$work/release" && sha256sum --quiet -c - <<< "$line")
  sum=${line%% *}
  echo "$platform: $file sha256:$sum, attested and in SHA256SUMS.txt"
  sums+=(--build-arg "SHA256_${arch^^}=$sum")
done

# The release's commit dates the image and names its revision.
commit="$(gh api "repos/$repo/commits/$tag" --jq '.sha + " " + .commit.committer.date')"
revision=${commit% *}
SOURCE_DATE_EPOCH="$(date -u -d "${commit#* }" +%s)"
export SOURCE_DATE_EPOCH
created="$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ)"

image=rewrite-timestamp=true
case "$how" in
  load) output=(--output "type=docker,name=$1,$image") ;;
  oci) output=(--output "type=oci,dest=$1,$image") ;;
  push)
    names="$(IFS=,; echo "$*")"
    output=(--output "type=image,\"name=$names\",push=true,unpack=false,oci-mediatypes=true,$image"
      --metadata-file "$work/metadata.json")
    ;;
esac

"$DOCKER" buildx build --file "$root/docker/Dockerfile" --platform "$platforms" \
  --provenance=false --sbom=false ${HYSCRIBE_IMAGE_NO_CACHE:+--no-cache} \
  --build-arg "VERSION=$version" "${sums[@]}" \
  --build-arg "REVISION=$revision" --build-arg "CREATED=$created" \
  "${output[@]}" "$work/context"

if [ "$how" = push ]; then
  sed -n 's/.*"containerimage.digest": *"\(sha256:[0-9a-f]*\)".*/\1/p' "$work/metadata.json"
fi

# The OCI archive's binary for each platform is the archive's, byte for
# byte; then its digest, the one `push` prints for the same inputs.
if [ "$how" = oci ]; then
  oci="$work/oci"
  mkdir "$oci"
  tar -xf "$1" -C "$oci"
  blob() { echo "$oci/blobs/sha256/${1#sha256:}"; }
  index="$(jq -r '.manifests[0].digest' "$oci/index.json")"
  for platform in ${platforms//,/ }; do
    arch=${platform#linux/}
    manifest="$(jq -r --arg arch "$arch" '.manifests[] | select(.platform.architecture == $arch) | .digest' "$(blob "$index")")"
    want="$(tar -xzOf "$work/release/hyscribe_${version}_linux_$arch.tar.gz" hyscribe/hyscribe | sha256sum | cut -d' ' -f1)"
    found=
    for layer in $(jq -r '.layers[].digest' "$(blob "$manifest")"); do
      if tar -tzf "$(blob "$layer")" usr/local/bin/hyscribe > /dev/null 2>&1; then
        found="$(tar -xzOf "$(blob "$layer")" usr/local/bin/hyscribe | sha256sum | cut -d' ' -f1)"
      fi
    done
    test "$found" = "$want" || { echo "$platform: the image's binary is not the archive's" >&2; exit 1; }
    size="$(jq '[.config.size, .layers[].size] | add' "$(blob "$manifest")")"
    echo "$platform: binary sha256:$want, the archive's; $((size / 1048576)) MiB compressed"
  done
  echo "$index"
fi
