FROM debian:bookworm-slim AS archive
ARG TARGETARCH
COPY image/${TARGETARCH}/ /archive/
RUN install -D /archive/dutchman /out/bin/dutchman \
    && install -D /archive/dutchman-mcp /out/bin/dutchman-mcp \
    && if [ -f /archive/dutchman-web ]; then install -D /archive/dutchman-web /out/bin/dutchman-web; fi \
    && mkdir -p /out/doc \
    && cp /archive/LICENSE /archive/NOTICE /out/doc/ \
    && cp -R /archive/licences /out/doc/

FROM debian:bookworm-slim AS slim
ARG VERSION=unknown
ARG LICENSES=Apache-2.0
LABEL org.opencontainers.image.source="https://github.com/dutchman-cad/dutchman" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.licenses="${LICENSES}"
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --create-home dutchman \
    && mkdir /work && chown 1000:1000 /work
COPY --from=archive /out/bin/ /usr/local/bin/
COPY --from=archive /out/doc/ /usr/share/doc/dutchman/
USER 1000:1000
WORKDIR /work
# For `dutchman-web` (docs/web.md): listen on every network, on port 8080.
ENV DUTCHMAN_WEB_LISTEN=0.0.0.0:8080
EXPOSE 8080
RUN dutchman --version
ENTRYPOINT ["dutchman"]
CMD ["--help"]

# The same image plus SolidCraft's `solidcraft-cli`, so each written history is checked. Built
# with the pin in docker/solidcraft.env; release-image.yml publishes it as X.Y.Z-solidcraft.
FROM slim AS solidcraft
ARG TARGETARCH
ARG SOLIDCRAFT_VERSION
ARG SOLIDCRAFT_SHA256_AMD64
ARG SOLIDCRAFT_SHA256_ARM64
USER root
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) arch=x86_64; sha256=$SOLIDCRAFT_SHA256_AMD64 ;; \
      arm64) arch=aarch64; sha256=$SOLIDCRAFT_SHA256_ARM64 ;; \
      *) echo "SolidCraft releases are for Linux x86_64 and ARM64" >&2; exit 1 ;; \
    esac; \
    if [ -z "$SOLIDCRAFT_VERSION" ] || [ -z "$sha256" ]; then echo "SolidCraft version and SHA-256 are needed" >&2; exit 1; fi; \
    archive="solidcraft-${SOLIDCRAFT_VERSION}-linux-${arch}.tar.gz"; \
    apt-get update; apt-get install -y --no-install-recommends curl; \
    curl -fsSL -o "/tmp/$archive" \
      "https://github.com/tangency-labs/solidcraft/releases/download/v${SOLIDCRAFT_VERSION}/$archive"; \
    echo "${sha256}  /tmp/$archive" | sha256sum -c -; \
    mkdir -p /opt/solidcraft; tar -xzf "/tmp/$archive" -C /opt/solidcraft --strip-components=1; \
    ln -s /opt/solidcraft/bin/solidcraft-cli /usr/local/bin/solidcraft-cli; \
    apt-get purge -y curl; apt-get autoremove -y; rm -rf /var/lib/apt/lists/* "/tmp/$archive"; \
    solidcraft-cli --version
USER 1000:1000

# A plain build, with no --target, is the slim image.
FROM slim AS default
