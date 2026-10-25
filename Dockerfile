# syntax=docker/dockerfile:1
#
# The `dutchman` command, the `dutchman-mcp` server and the `dutchman-web` browser interface,
# without the desktop app. docs/docker.md says how to build and run it.

FROM rust:1-bookworm AS build
# How many crates to compile at once (default: one per core). Each can take more than 1 GB, so
# lower it where the build runs short of memory, for example --build-arg CARGO_BUILD_JOBS=2.
ARG CARGO_BUILD_JOBS=
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p dutchman -p dutchman-web --bins ${CARGO_BUILD_JOBS:+--jobs "$CARGO_BUILD_JOBS"} \
 && install -m 0755 target/release/dutchman target/release/dutchman-mcp target/release/dutchman-web /usr/local/bin/

FROM debian:bookworm-slim
ARG TARGETARCH
# Optional: a SolidCraft release whose `solidcraft-cli` checks each written history
# (Linux x86_64 only). Give both, for example --build-arg SOLIDCRAFT_VERSION=0.20.0
# --build-arg SOLIDCRAFT_SHA256=<the release archive's SHA-256>.
ARG SOLIDCRAFT_VERSION=
ARG SOLIDCRAFT_SHA256=

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

RUN --mount=type=bind,from=build,source=/usr/local/bin,target=/mnt/bin set -eu; \
    install -m 0755 /mnt/bin/dutchman /mnt/bin/dutchman-mcp /mnt/bin/dutchman-web /usr/local/bin/; \
    if [ -n "$SOLIDCRAFT_VERSION" ]; then \
      if [ "$TARGETARCH" != amd64 ]; then echo "SolidCraft releases are for Linux x86_64 only" >&2; exit 1; fi; \
      if [ -z "$SOLIDCRAFT_SHA256" ]; then echo "SOLIDCRAFT_SHA256 is needed with SOLIDCRAFT_VERSION" >&2; exit 1; fi; \
      apt-get update; apt-get install -y --no-install-recommends curl; \
      archive="solidcraft-${SOLIDCRAFT_VERSION}-linux-x86_64.tar.gz"; \
      curl -fsSL -o "/tmp/$archive" "https://github.com/tangency-labs/solidcraft/releases/download/v${SOLIDCRAFT_VERSION}/$archive"; \
      echo "${SOLIDCRAFT_SHA256}  /tmp/$archive" | sha256sum -c -; \
      mkdir -p /opt/solidcraft; tar -xzf "/tmp/$archive" -C /opt/solidcraft --strip-components=1; \
      ln -s /opt/solidcraft/bin/solidcraft-cli /usr/local/bin/solidcraft-cli; \
      apt-get purge -y curl; apt-get autoremove -y; rm -rf /var/lib/apt/lists/* "/tmp/$archive"; \
    fi; \
    dutchman --version

RUN useradd --create-home --uid 1000 dutchman
USER dutchman
WORKDIR /work
# For `dutchman-web` (docs/web.md): listen on every network, on port 8080.
ENV DUTCHMAN_WEB_LISTEN=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["dutchman"]
CMD ["--help"]
