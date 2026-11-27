# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The container image (ADR 0005, amendment of 2026-11-27): the released
# binary, unchanged, on distroless's static base. Built from the release
# archives, never by compiling here, by scripts/docker-image.sh:
#   scripts/docker-image.sh <dist folder> <image name>
# The build context holds the archives only. Nothing runs during the build
# (no RUN), so the arm64 image is assembled on an amd64 machine without
# emulation.
#
# The base, pinned by its multi-architecture index: CA certificates (with
# their licence), /etc/passwd with the nonroot user (65532), /tmp, and no
# shell or package manager.
FROM gcr.io/distroless/static-debian13:nonroot@sha256:8ce6dd87e9823c5b48dcfb2c8604714ccb351ce59607327bcc4957448da279b5 AS base

# ADD unpacks a local archive without running anything.
FROM scratch AS archive
ARG TARGETARCH
ARG VERSION
ADD hyscribe_${VERSION}_linux_${TARGETARCH}.tar.gz /

FROM base
ARG VERSION
ARG REVISION
ARG CREATED
COPY --from=archive /hyscribe/hyscribe /usr/local/bin/hyscribe
COPY --from=archive /hyscribe/LICENSE /hyscribe/NOTICE /hyscribe/THIRD-PARTY-NOTICES /hyscribe/README.txt /hyscribe/.env.example /usr/share/doc/hyscribe/
COPY --from=archive /hyscribe/docs/ /usr/share/doc/hyscribe/docs/
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
      org.opencontainers.image.base.name="gcr.io/distroless/static-debian13:nonroot" \
      org.opencontainers.image.base.digest="sha256:8ce6dd87e9823c5b48dcfb2c8604714ccb351ce59607327bcc4957448da279b5"
