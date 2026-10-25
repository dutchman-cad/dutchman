### Command line and install

- Each release is also published as a container image, `ghcr.io/dutchman-cad/dutchman`, for
  amd64 and arm64, tagged with its version and `latest` (nightly builds as `nightly`). It holds
  the same binaries as the Linux downloads; [docs/docker.md](docs/docker.md) covers pulling,
  converting a mounted folder, the MCP server, and updating. Linux downloads are built on
  Debian bookworm, so they run on systems with glibc 2.36 or newer.
