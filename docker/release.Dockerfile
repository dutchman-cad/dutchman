FROM debian:bookworm-slim AS archive
ARG TARGETARCH
COPY image/${TARGETARCH}/ /archive/
RUN install -D /archive/dutchman /out/bin/dutchman \
    && install -D /archive/dutchman-mcp /out/bin/dutchman-mcp \
    && if [ -f /archive/dutchman-web ]; then install -D /archive/dutchman-web /out/bin/dutchman-web; fi \
    && mkdir -p /out/doc \
    && cp /archive/LICENSE /archive/NOTICE /out/doc/ \
    && cp -R /archive/licences /out/doc/

FROM debian:bookworm-slim
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
