# Docker

Dutchman runs in a container with the `dutchman` command, the `dutchman-mcp`
[MCP server](mcp.md) and the `dutchman-web` [browser interface](web.md), for x86_64 and ARM64
Linux. It has no desktop app: `dutchman-gui` needs a desktop session. Use it to convert on a
server, in CI, or anywhere you would rather not install Dutchman.

## Get the image

### Pull the published image

Each release publishes `ghcr.io/dutchman-cad/dutchman`, made from the same Linux binaries as the
downloads; Docker picks the image for its own architecture:

```sh
docker pull ghcr.io/dutchman-cad/dutchman:latest
docker tag ghcr.io/dutchman-cad/dutchman:latest dutchman
docker run --rm dutchman --version
```

The `docker tag` line lets the commands below say `dutchman`; you can also give the full name
each time. The tags are:

- `X.Y.Z`: one release, such as `0.2.1`; it never changes.
- `latest`: the newest release.
- `X.Y.Z-solidcraft` and `latest-solidcraft`: the same release with SolidCraft's checker added,
  so histories are checked ([below](#checking-histories-in-solidcraft)).
- `nightly`: built each day from `main` against cadmpeg's `main`, after its tests pass.

To update, pull again (`docker pull ghcr.io/dutchman-cad/dutchman:latest`, then tag it again),
or with Compose, `docker compose pull && docker compose up -d`. A tool such as Watchtower can
pull new releases for you.

### Build it yourself

To build from a checkout of the repository instead, with the [`Dockerfile`](../Dockerfile) at its top:

```sh
docker build -t dutchman .
```

It builds `dutchman` from that checkout in a Rust container, then copies the three programs into
a small Debian image. The first build takes several minutes; later builds reuse the downloaded
crates. It compiles one crate per core at once, and the largest need several GB of memory each:
if the build stops with `cannot allocate memory` or `SIGKILL`, compile fewer at once, or give
Docker more memory. With about 8 GB for Docker, compile one at a time; on an Apple silicon Mac
that build took about 45 minutes:

```sh
docker build -t dutchman --build-arg CARGO_BUILD_JOBS=1 .
```

Check it:

```sh
docker run --rm dutchman --version
```

## Convert

The image runs `dutchman` in `/work`. Mount the folder that holds your designs there, and
run as yourself so that the converted designs belong to you:

```sh
docker run --rm -v "$PWD:/work" --user "$(id -u):$(id -g)" dutchman part.f3d
docker run --rm -v "$PWD:/work" --user "$(id -u):$(id -g)" dutchman convert --json part.f3d out.sc
docker run --rm -v "$PWD:/work" --user "$(id -u):$(id -g)" dutchman convert-all --json --out-dir converted/ project/
```

Paths are inside the container, so give them relative to the mounted folder. Every command,
option and exit code is the same as outside Docker ([Getting started](guide.md)), and `--json`
prints the same reports ([json.md](json.md)).

An Inventor assembly needs its part files: mount the whole project folder, not just the
assembly's own folder, or the parts it cannot find are reported as left out.

`convert-all` stops a file that runs past `--timeout` or holds more than `--max-memory` (no
limit by default). A file also stops when the container's own memory limit is nearly reached, so
the container isn't stopped first.

## In a browser

The repository's [`compose.yaml`](../compose.yaml) runs `dutchman-web` from the image tagged
`dutchman` on port 8080, asking for the password you give it. With the published image pulled
and tagged as above:

```sh
DUTCHMAN_WEB_PASSWORD=choose-one docker compose up -d
```

To build the image from the checkout instead, add `--build`; the first build takes several
minutes or more, as in [Build it yourself](#build-it-yourself).

Open `http://<server>:8080` and sign in as `dutchman`. Or run it with `docker run`:

```sh
docker run -d --name dutchman-web -p 8080:8080 -e DUTCHMAN_WEB_PASSWORD=choose-one \
  --entrypoint dutchman-web dutchman
```

When exposing the port beyond the local network, put it behind an HTTPS reverse proxy and
keep port 8080 private to the proxy. HTTP basic authentication over plain HTTP sends the
password readably. Preserve the request's `Host` header or set `DUTCHMAN_WEB_PUBLIC_ORIGIN`
to the proxy's public origin, such as `https://cad.example.org`.

[Dutchman Web](web.md) describes the page, its upload and disk limits, and its settings.

## As an MCP server

Run `dutchman-mcp` in the container, with stdin kept open (`-i`). Mount your designs at the
same path inside the container as outside it, because the tools take absolute paths on the
machine running the server. For Claude Code, with designs under `/home/me/cad`:

```sh
claude mcp add dutchman -- docker run --rm -i -v /home/me/cad:/home/me/cad --user 1000:1000 --entrypoint dutchman-mcp dutchman
```

Other clients take the same command in their `mcpServers` entry:

```json
{
  "mcpServers": {
    "dutchman": {
      "command": "docker",
      "args": ["run", "--rm", "-i", "-v", "/home/me/cad:/home/me/cad", "--user", "1000:1000",
               "--entrypoint", "dutchman-mcp", "dutchman"]
    }
  }
}
```

Replace `/home/me/cad` with your folder and `1000:1000` with your user and group ids
(`id -u`, `id -g`).

## With Compose

### SolidCraft and Dutchman on one address

The [Compose example](../examples/compose/) serves web SolidCraft at `/` and Dutchman Web at
`/dutchman/` behind Caddy, with a shared folder and a watched inbox. Both apps come from
published images, so it starts in about a minute. Copy the folder to your server and follow its
[README](../examples/compose/README.md): create the folders and `.env`, then run
`docker compose up -d`. Each converted design's card gets an **Open in SolidCraft** link.

### Convert from the command line

Save as `compose.yaml` next to your designs, then convert with
`docker compose run --rm dutchman convert-all --out-dir converted/ .`:

```yaml
services:
  dutchman:
    image: ghcr.io/dutchman-cad/dutchman:latest
    user: "${LOCAL_UID:-1000}:${LOCAL_GID:-1000}"
    working_dir: /work
    volumes:
      - .:/work
```

Set `LOCAL_UID=$(id -u) LOCAL_GID=$(id -g)` first so the converted designs belong to you. On a
host with SELinux, add `:Z` to the volume (`.:/work:Z`).

## Checking histories in SolidCraft

Without `solidcraft-cli`, a written history is reported `unverified`
([Feature history](history.md)), and a few features that need SolidCraft's replay to find their
edges (such as some FreeCAD fillets) are not mapped. The default image has no `solidcraft-cli`,
to stay small. To get checked histories with no build step, use the image that includes it:

```sh
docker pull ghcr.io/dutchman-cad/dutchman:latest-solidcraft
docker tag ghcr.io/dutchman-cad/dutchman:latest-solidcraft dutchman
docker run --rm --entrypoint solidcraft-cli dutchman --version
```

It is the same Dutchman for x86_64 and ARM64, plus the SolidCraft release that Dutchman targets
(0.46 for now), and adds about 550 MB. Pin a release with `X.Y.Z-solidcraft`, such as
`0.2.1-solidcraft`. In Compose, use `image: ghcr.io/dutchman-cad/dutchman:latest-solidcraft`.

### Build your own

To check histories with a different SolidCraft release, add that
[release](https://github.com/tangency-labs/solidcraft/releases) to the image yourself. Give its
version and the SHA-256 of its `solidcraft-VERSION-linux-x86_64.tar.gz`, or
`-linux-aarch64.tar.gz` on ARM64, shown beside each file on the release page. The build stops if
the download does not match.

To add it to the default image without building Dutchman, save this as `Dockerfile` in an
empty folder:

```dockerfile
FROM ghcr.io/dutchman-cad/dutchman:latest
ARG TARGETARCH
ARG SOLIDCRAFT_VERSION
ARG SOLIDCRAFT_SHA256
USER root
RUN set -eu; \
    case "$TARGETARCH" in amd64) arch=x86_64 ;; arm64) arch=aarch64 ;; *) exit 1 ;; esac; \
    archive="solidcraft-${SOLIDCRAFT_VERSION}-linux-${arch}.tar.gz"; \
    apt-get update; apt-get install -y --no-install-recommends curl; \
    curl -fsSL -o "/tmp/$archive" \
      "https://github.com/tangency-labs/solidcraft/releases/download/v${SOLIDCRAFT_VERSION}/$archive"; \
    echo "${SOLIDCRAFT_SHA256}  /tmp/$archive" | sha256sum -c -; \
    mkdir -p /opt/solidcraft; tar -xzf "/tmp/$archive" -C /opt/solidcraft --strip-components=1; \
    ln -s /opt/solidcraft/bin/solidcraft-cli /usr/local/bin/solidcraft-cli; \
    apt-get purge -y curl; apt-get autoremove -y; rm -rf /var/lib/apt/lists/* "/tmp/$archive"
USER 1000:1000
```

Then build it in that folder, with the SolidCraft 0.46.2 archive for x86_64 here, and use
`dutchman` as before:

```sh
docker build -t dutchman \
  --build-arg SOLIDCRAFT_VERSION=0.46.2 \
  --build-arg SOLIDCRAFT_SHA256=b04255a539a4e060b98087ba70f29538b84f0f7cc0178fc3a2167926d0a9178c .
```

It takes about a minute and adds about 550 MB. Build again after each `docker pull` to update
Dutchman. An image you [build yourself](#build-it-yourself) from the repository takes the same
two arguments.

## What the image leaves out

- **The desktop app.** Run `dutchman-gui` on a computer with a desktop.
- **FreeCAD.** `--to freecad` writes its macro without FreeCAD, but `--to fcstd` and
  `--verify` need FreeCAD's command-line program, which the image does not include. Build
  your own image on top of this one with FreeCAD installed, and set `FREECAD_CMD` to it.

Merge queue and full CI Docker smoke tests reuse the optimized, non-LTO Bookworm
programs built for placements with `docker/release.Dockerfile`. The standalone Docker
workflow continues to build the source `Dockerfile`, including its Rust 1.88 builder.

Merge groups also build the source `Dockerfile` when either Docker recipe, `.dockerignore`,
Rust source/manifests, runtime files, or another potential build input changes. Unknown
paths or an unavailable diff base require the source build. A pushed tag has no previous
commit, so it is compared with the nearest earlier release tag and with where it left main: when
either shows only docs, Rust sources or CI configuration changed, the tag takes main's fast path.
Docs/support-only changes retain the fast image path. CI's Docker job reports its result but
doesn't fail CI or hold a release: before a release image is pushed, the release workflow builds
it from the published archives and checks that it converts a design and starts its MCP and web
servers, and pushes nothing if any check fails.

The source image’s Rust build stops after 90 minutes, including dependency downloads. Cargo uses a
60-second HTTP timeout and up to three network retries. Optional SolidCraft downloads use
a 15-second connection timeout, a five-minute transfer timeout, and up to three retries
within a ten-minute retry window.

In CI, the source image step has a 100-minute deadline. Layer cache transfers have a
five-minute deadline and export only final-image layers. Each main commit runs independently,
so a stuck run does not hold up checks for newer commits. Pull requests and merge groups
keep their existing cancellation behavior.
