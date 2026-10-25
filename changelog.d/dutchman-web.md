### Command line and install

- Convert from a browser with `dutchman-web`, a web server for a self-hosted Dutchman: drop
  design files or a project zip, read each file's report, and download the designs. Uploads are
  converted as `convert-all` converts them and deleted after a while; a password and upload
  limits are settings. The Docker image includes it, and `compose.yaml` runs it on port 8080
  ([docs/web.md](docs/web.md)).
