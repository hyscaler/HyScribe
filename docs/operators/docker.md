# Docker

The container image is a second way to run HyScribe, for trying it with Docker on Linux, macOS or Windows. It is optional: the release archives ([install.md](install.md)) stay the main way to install HyScribe, and nothing in HyScribe needs Docker.

Each published release gets one image for `linux/amd64` and `linux/arm64` (Docker picks the right one), holding that release's Linux binary, byte for byte the one in its archive. On macOS and Windows it runs under Docker Desktop, natively on Apple Silicon and on x86_64 or arm64 PCs. Docker Engine and Podman run it on Linux.

## The image

- **Name:** `docker.io/hyscribe/hyscribe` on Docker Hub, and the same image as `ghcr.io/hyscaler/hyscribe` on GitHub Container Registry. Both carry the same digests
- **Tags:** the version (`0.2.0`), the release line (`0.2`) and `latest`, from the release after v0.1.0 (v0.1.0's binary predates `HYSCRIBE_HOME`, below, so it gets no image). `latest` moves to a release when it is GitHub's latest one, so rebuilding an older release leaves it. A pre-release gets its version tag only
- **Architectures:** `linux/amd64` and `linux/arm64` under every tag
- **Base:** Debian 13's `debian:trixie-slim` (pinned by digest), with Debian's CA certificates added for outbound HTTPS and the mail relay's TLS, and the `nonroot` user. It has Debian's shell and tools, but nothing HyScribe needs besides the binary
- **User:** `nonroot`, uid and gid 65532. HyScribe refuses to run as root, in a container too
- **Data:** the installation's folder is `/hyscribe`, a volume. `.env`, `logs/`, `sessions/`, `run/`, `uploads/` (a new site's media), `plugins/`, `packages/`, `registry/` and the rest of [DEPLOYMENT.md](../DEPLOYMENT.md)'s layout are written there, as in the `hyscribe/` folder of an archive install. The binary is `/usr/local/bin/hyscribe`, outside the volume, so a new image is a new binary; `HYSCRIBE_HOME=/hyscribe` tells it where the installation is
- **Port:** 8080. The image sets `BIND_HOST=0.0.0.0` so the container's port can be published; publish it on `127.0.0.1` (below) unless you put a proxy in front
- **Command:** `hyscribe start`, in the foreground. Other commands run in place of it: `docker run --rm docker.io/hyscribe/hyscribe version`
- **Health check:** `hyscribe service status`, which answers 0 while the server holds its lock and runs. It says the process is up, not that the database answers
- **Licences:** `LICENSE`, `NOTICE`, `THIRD-PARTY-NOTICES`, `README.txt`, `.env.example` and these docs are in `/usr/share/doc/hyscribe/`; Debian's packages keep their copyright files under `/usr/share/doc/`
- **Provenance:** each image is attested by the workflow that built it: `gh attestation verify oci://ghcr.io/hyscaler/hyscribe:<version> --repo hyscaler/HyScribe`

## How the image is made

The image is built when a release is published, never from source: the workflow `docker.yml` downloads the release's `hyscribe_<version>_linux_amd64.tar.gz` and `hyscribe_<version>_linux_arm64.tar.gz` from GitHub, checks each one's build attestation and its line in the release's `SHA256SUMS.txt`, and passes each SHA-256 to [docker/Dockerfile](../../docker/Dockerfile), whose build downloads the archive again and stops if it differs. Before pushing, it builds the image twice and compares the digests, checks that each platform's `/usr/local/bin/hyscribe` is byte for byte its archive's binary, and runs the amd64 image against a test database as an existing and as a new site.

### Rebuilding the image of a release

Anyone can build the same image from the repository, with Docker's buildx, an image store that keeps multi-platform images (Docker Desktop, or Docker Engine's containerd store) and an authenticated GitHub CLI:

```bash
scripts/docker-image.sh v0.2.0 oci hyscribe-0.2.0.tar   # both platforms; checks the binaries, prints the digest
scripts/docker-image.sh v0.2.0 load hyscribe:0.2.0      # amd64 only, into the local image store
```

The script does the release's checks first. By hand, the same build is:

```bash
gh release download v0.2.0 --repo hyscaler/HyScribe --pattern SHA256SUMS.txt
gh attestation verify SHA256SUMS.txt --repo hyscaler/HyScribe
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct v0.2.0) docker buildx build \
  --platform linux/amd64,linux/arm64 --provenance=false --sbom=false \
  --build-arg VERSION=0.2.0 \
  --build-arg SHA256_AMD64=$(awk '$2 == "hyscribe_0.2.0_linux_amd64.tar.gz" {print $1}' SHA256SUMS.txt) \
  --build-arg SHA256_ARM64=$(awk '$2 == "hyscribe_0.2.0_linux_arm64.tar.gz" {print $1}' SHA256SUMS.txt) \
  --build-arg REVISION=$(git rev-parse 'v0.2.0^{commit}') \
  --build-arg CREATED=$(date -u -d @"$(git log -1 --format=%ct v0.2.0)" +%Y-%m-%dT%H:%M:%SZ) \
  --output type=oci,dest=hyscribe-0.2.0.tar,rewrite-timestamp=true -f docker/Dockerfile docker
```

The base images are pinned by digest and the timestamps are the release commit's, so the rebuild gives the published digest as long as Debian's `ca-certificates` package is the same one; after a Debian update of that package the CA bundle, and so the digest, differ while the HyScribe binary stays the same bytes. Every build step runs on the build host's own platform, so an amd64 machine builds the arm64 image without emulation.

## Try it with Compose

`docker/compose.yaml` in the repository runs HyScribe with a MariaDB 11.8, each with its data in a named volume, HyScribe on `127.0.0.1:8080`. Copy `docker/compose.yaml` (and `compose.existing.yaml` for an existing site) into an empty folder, and put a `.env` file beside it with a new database password. Compose reads that `.env` itself; it is not HyScribe's `.env`.

```bash
printf 'DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" > .env
```

In PowerShell 7: `"DB_PASSWORD=" + [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(24)) | Set-Content .env`. Keep the file private; nothing in the repository holds a password.

`.env` may also set `HYSCRIBE_PORT` (8080), `HYSCRIBE_ADDRESS` (`127.0.0.1`; `0.0.0.0` publishes it on every interface), `HYSCRIBE_IMAGE` (another tag or registry), and `DB_NAME` and `DB_USER` (both `wordpress`).

### A new site

```bash
docker compose up -d
docker compose cp hyscribe:/hyscribe/logs/setup-secret .
cat setup-secret        # Windows: type setup-secret
```

HyScribe starts in setup mode and writes the one-time setup secret into its volume, which `docker compose cp` copies out (`docker compose exec hyscribe cat /hyscribe/logs/setup-secret` shows it too). Open `http://127.0.0.1:8080/`, enter the secret, tick "This is a new site", and answer the database questions with host `db`, port 3306, database `wordpress`, user `wordpress` and the password from `.env`, then the site title, the administrator and the site's address (`http://127.0.0.1:8080`). HyScribe installs WordPress 7.1.3's tables and first content ([ADR 0021](../adr/0021-new-site-install.md)), writes `/hyscribe/.env` and serves the site straight away, in primary mode. Delete the `setup-secret` copy; it no longer works anyway.

The terminal wizard does the same with no browser: `docker compose run --rm -it hyscribe setup` before the first `up`, answering `new` to the first question.

Setup mode stops after 30 minutes without `.env`: `docker compose restart hyscribe` gives a new secret.

### A copy of an existing site

Use a copy, never the live site's database or files: in primary mode HyScribe writes to both. Make a dump (`mysqldump --single-transaction` or `wp db export`, plain SQL, not compressed) and copy `wp-content`, then name them in `.env`:

```
DB_PASSWORD=...
SITE_DUMP=/path/to/copy/site.sql
SITE_WP_CONTENT=/path/to/copy/wp-content
DB_PREFIX=wp_
WP_HOME=http://127.0.0.1:8080
```

```bash
docker compose -f compose.yaml -f compose.existing.yaml up -d
```

The database loads the dump the first time its volume is created (it takes a while for a large site; `docker compose logs db` shows it). HyScribe is configured by the environment, with no wizard, and starts in shadow mode (read only): set `MODE=primary` in `.env` to write. `WP_HOME` points the site's links at the trial address instead of the live domain. The keys and salts of the site's `wp-config.php` (`AUTH_KEY` to `NONCE_SALT`) may be added to `.env` too; without them HyScribe uses none, and logging in needs primary mode in any case. The copy of `wp-content` is mounted read and write at `/wordpress/wp-content`, its `uploads/` as `UPLOADS_DIR`. On Linux the container's user (65532) must be able to read it, and write to `uploads/` in primary mode; Docker Desktop maps the permissions itself.

## Without Compose

```bash
docker volume create hyscribe
docker run -d --name hyscribe -p 127.0.0.1:8080:8080 -v hyscribe:/hyscribe \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  -e DB_HOST=db.example.internal -e DB_NAME=wordpress -e DB_USER=wordpress -e DB_PASSWORD \
  docker.io/hyscribe/hyscribe:latest
```

Every key of [configuration.md](configuration.md) can be given with `-e`, and real environment variables win over the volume's `.env`, key by key. Without `DB_NAME` and with no `.env` in the volume, HyScribe starts in setup mode, as above (`docker cp hyscribe:/hyscribe/logs/setup-secret .`). A database on the Docker host itself is `host.docker.internal` under Docker Desktop, or the host's address on the bridge network under Docker Engine.

## Logs

`hyscribe start` logs to standard output: `docker compose logs -f hyscribe` or `docker logs -f hyscribe`. `logs/hyscribe.log` and `hyscribe service log` belong to `service start` and stay empty in a container.

## Upgrading

```bash
docker compose pull && docker compose up -d
```

The volume keeps `.env` and everything else; the new container runs the new binary on it, as an archive upgrade replaces the binary and keeps the folder ([maintenance.md](maintenance.md)). Pin a version tag in `HYSCRIBE_IMAGE` to choose when to upgrade.

## Backups

The database is backed up as for any site ([maintenance.md](maintenance.md)): `docker compose exec db mariadb-dump --single-transaction -u wordpress -p wordpress > site.sql`. The HyScribe volume holds `.env` (credentials, keys and salts), sessions, logs and a new site's uploads. Copy it out with a throwaway container while HyScribe is stopped:

```bash
docker compose stop hyscribe
docker run --rm -v hyscribe_hyscribe:/hyscribe:ro -v "$PWD":/backup busybox tar -czf /backup/hyscribe-volume.tar.gz -C /hyscribe .
docker compose start hyscribe
```

Compose names the volumes after the project folder (`docker volume ls`). Keep the archive as private as `.env`. Restore by extracting it into a new volume with the same kind of container, owned by 65532 (`tar -xzpf`, run as root in that container, keeps the owners).

## Security

- **Not root:** the server runs as uid 65532 and refuses uid 0, so `--user 0` does not start it
- **Read only root filesystem:** works, as the Compose file sets it (`read_only: true`); HyScribe writes only to `/hyscribe` and the mounted `wp-content`
- **No capabilities:** none are needed (`cap_drop: ALL`, `no-new-privileges`); port 8080 is above 1024
- **No Docker socket:** nothing in the image talks to Docker; never mount `/var/run/docker.sock` into it
- **Localhost by default:** the Compose file publishes the port on `127.0.0.1` only. For a site others reach, put a reverse proxy with HTTPS in front and set `BEHIND_PROXY=true` with `TRUSTED_PROXIES` naming the proxy's address on the Docker network ([security.md](security.md))
- **Client addresses:** the login limits count wrong passwords by client address. Docker's port forwarding may show every visitor as one address of the bridge network, which then shares one limit; a proxy that forwards the client's address, trusted with `TRUSTED_PROXIES`, avoids it
- **Secrets:** pass passwords through Compose's `.env`, `-e` or a secret store; they are never written into the image

## Not supported in Docker

- `hyscribe service start`, `stop`, `restart` and `log`: the container is the service. `docker compose stop` sends SIGTERM, which the server handles as `service stop` does
- Starting at boot is Docker's job (`restart: unless-stopped` in the Compose file)
- Windows and macOS images: there are none, and none are needed; Docker Desktop runs the Linux image
- The arm64 image's build and its binary are checked before each push, but only the amd64 image is run against a test database
