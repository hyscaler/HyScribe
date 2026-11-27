# syntax=docker/dockerfile:1.28.0@sha256:bb22d9815c728170f72750f4e5b0d672e06176142e1d602c7e66c050100b7e5b
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The container image (ADR 0005, amendment of 2026-11-27): a published
# release's Linux binary, unchanged, on Debian's trixie-slim. The archive is
# downloaded from the GitHub release and checked against its SHA-256, never
# compiled here. scripts/docker-image.sh verifies the release (attestation
# and SHA256SUMS.txt) and passes the build arguments; by hand:
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     --build-arg VERSION=0.1.0 --build-arg SHA256_AMD64=<sum> --build-arg SHA256_ARM64=<sum> \
#     --build-arg REVISION=<commit> --build-arg CREATED=<date> -f docker/Dockerfile docker
# Every RUN runs on the build machine's own platform, and the target's stage
# only copies, so the arm64 image is assembled on amd64 without emulation.

# Debian trixie-slim, pinned by its multi-architecture index.
ARG BASE=debian:trixie-slim@sha256:8aa5a7bf5a1955bddbe7d74fcdd0b91ab30e74362a8b73af672b0481b35b44a0

# The release archive of each architecture; a different download fails here.
FROM scratch AS archive-amd64
ARG VERSION
ARG SHA256_AMD64
ADD --checksum=sha256:${SHA256_AMD64} https://github.com/hyscaler/HyScribe/releases/download/v${VERSION}/hyscribe_${VERSION}_linux_amd64.tar.gz /hyscribe.tar.gz

FROM scratch AS archive-arm64
ARG VERSION
ARG SHA256_ARM64
ADD --checksum=sha256:${SHA256_ARM64} https://github.com/hyscaler/HyScribe/releases/download/v${VERSION}/hyscribe_${VERSION}_linux_arm64.tar.gz /hyscribe.tar.gz

FROM archive-${TARGETARCH} AS archive

# The target platform's base, read below and never run.
FROM ${BASE} AS target

# On the build platform: the archive unpacked, the CA bundle the binary
# reads (/etc/ssl/certs/ca-certificates.crt), and the target's passwd and
# group with the nonroot user, 65532, added.
FROM --platform=$BUILDPLATFORM ${BASE} AS files
RUN --mount=from=archive,target=/archive --mount=from=target,target=/target \
    apt-get update -qq \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends ca-certificates >/dev/null \
    && mkdir -p /out/etc/ssl/certs /out/usr/share/doc/ca-certificates /out/usr/local/bin /out/usr/share/doc/hyscribe \
    && cp /etc/ssl/certs/ca-certificates.crt /out/etc/ssl/certs/ \
    && cp /usr/share/doc/ca-certificates/copyright /out/usr/share/doc/ca-certificates/ \
    && cp /target/etc/passwd /target/etc/group /out/etc/ \
    && echo 'nonroot:x:65532:65532:nonroot:/hyscribe:/usr/sbin/nologin' >> /out/etc/passwd \
    && echo 'nonroot:x:65532:' >> /out/etc/group \
    && tar -xzf /archive/hyscribe.tar.gz -C /tmp \
    && mv /tmp/hyscribe/hyscribe /out/usr/local/bin/hyscribe \
    && mv /tmp/hyscribe/LICENSE /tmp/hyscribe/NOTICE /tmp/hyscribe/THIRD-PARTY-NOTICES \
          /tmp/hyscribe/README.txt /tmp/hyscribe/.env.example /tmp/hyscribe/docs /out/usr/share/doc/hyscribe/

FROM target
ARG VERSION
ARG REVISION
ARG CREATED
COPY --from=files /out/ /
ENV HYSCRIBE_HOME=/hyscribe \
    BIND_HOST=0.0.0.0 \
    PORT=8080
USER 65532:65532
# The installation's folder, empty and made by the nonroot user, so a new
# volume mounted on it is the user's too. HYSCRIBE_HOME puts .env, logs/,
# uploads/, plugins/ and the rest there; the binary stays in the image.
WORKDIR /hyscribe
VOLUME ["/hyscribe"]
EXPOSE 8080
# Holding the pid file's lock is what running means (docs/DEPLOYMENT.md).
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD ["/usr/local/bin/hyscribe", "service", "status"]
ENTRYPOINT ["/usr/local/bin/hyscribe"]
CMD ["start"]
LABEL org.opencontainers.image.title="HyScribe" \
      org.opencontainers.image.description="Rust drop-in replacement for WordPress" \
      org.opencontainers.image.source="https://github.com/hyscaler/HyScribe" \
      org.opencontainers.image.url="https://github.com/hyscaler/HyScribe" \
      org.opencontainers.image.documentation="https://github.com/hyscaler/HyScribe/blob/main/docs/operators/docker.md" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later" \
      org.opencontainers.image.vendor="HyScaler" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}" \
      org.opencontainers.image.base.name="docker.io/library/debian:trixie-slim" \
      org.opencontainers.image.base.digest="sha256:8aa5a7bf5a1955bddbe7d74fcdd0b91ab30e74362a8b73af672b0481b35b44a0"
