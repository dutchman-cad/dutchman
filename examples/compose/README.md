# SolidCraft and Dutchman on one address

This example puts three services behind Caddy, with one shared folder:

- SolidCraft at `/`.
- Dutchman Web at `/dutchman/`. Sign in as `dutchman` with your password.
- A watcher that converts files dropped in `Inbox` without a browser.

Converted designs land in the share's `Converted` folder. On a web result card,
**Open in SolidCraft** opens that result directly at `/?open=/dutchman/jobs/...`.
The link works while Dutchman keeps the job (one hour by default). The copy in
`Converted` stays after the job expires. SolidCraft keeps nothing on the server;
save browser edits to a file yourself.

## Set up

Use Docker with Compose. Copy this folder to
your server, then change into the copy. Create the folders as the user who owns
the designs:

```sh
mkdir -p designs/Inbox designs/Converted
cat > .env <<EOF
UID=$(id -u)
GID=$(id -g)
DUTCHMAN_WEB_PASSWORD=replace-with-a-long-password
EOF
chmod 600 .env
```

Edit `.env` to set your password before starting. The UID and GID must have write
access to both folders. To use a NAS, mount the share on the host first, add
`DESIGNS=/mnt/share/designs` to `.env`, and create `Inbox` and `Converted` there.
Check the share's ownership and permissions for the configured UID and GID.
On SELinux hosts, use a shared label (`:z`) on both `/designs` mounts.

```sh
docker compose up -d
```

The first start downloads the images. Open `http://localhost:8080`
on the server, or `http://<server>:8080` from another computer. Set `PORT` in `.env`
to change the port. Only Caddy publishes a port.

Drop native CAD files or project folders into `Inbox`. The watcher waits for files
to settle before converting, and keeps relative folders in `Converted`. Keep the
hidden `.dutchman-watch.json` state file with the outputs, and run only one watcher
per output folder. Web uploads also save to `Converted`, numbering names already
taken. See [Watch a folder](../../docs/command-line.md#watch-a-folder) and
[Save to a shared folder](../../docs/web.md#save-to-a-shared-folder).

Each Dutchman container has a 10 GB memory limit. One file may use 8 GB, and the
defaults allow several files at once. For a small server, add `--runs 1 --jobs 1`
to the web command and `--jobs 1` to the watch command, and lower `--max-memory`
in both commands as needed. Budget for both services running at once.

## HTTPS

The browser saves designs only over HTTPS when used from another computer.
Localhost is the exception. HTTP basic authentication over plain HTTP sends the
password readably with every request. Use HTTPS before signing in over an
untrusted network.

For a LAN name, replace the HTTP site with the commented `cad.home.arpa` block in
`Caddyfile`. Make that name resolve to the server. Change the published HTTP port
to `80:80` and enable `443:443` in `compose.yaml`. Caddy's `tls internal` issues a
local certificate. Copy its public root certificate from
`/data/caddy/pki/authorities/local/root.crt` in the proxy container, and trust it on
each computer that uses the site. Keep the `caddy_data` volume across updates.
Then open `https://cad.home.arpa`.

For a Tailscale HTTPS name, enable HTTPS in your tailnet and use Tailscale Serve
on the host to forward that name to `http://127.0.0.1:8080`. Keep the default
HTTP Caddyfile, and bind its published port to `127.0.0.1:8080:80` instead of
`${PORT:-8080}:80`. Open the site's `https://<machine>.<tailnet>.ts.net` address.

Caddy preserves `Host` and passes `/dutchman` through unchanged. If another proxy
changes `Host`, set `DUTCHMAN_WEB_PUBLIC_ORIGIN` in `.env` to the exact browser
origin, including scheme and any port, then recreate the services.

## Update

```sh
docker compose pull
docker compose up -d
```

This updates Caddy within its pinned 2.x minor, and SolidCraft and Dutchman to
their newest releases. The example needs Dutchman 0.2.2 or later for
`--base-path`, `--output-dir`, and `--solidcraft`, and SolidCraft 0.24.0 or later
for opening a result on the same origin. To hold Dutchman at one version, set
both Dutchman images to that version instead of `latest` (`X.Y.Z-solidcraft` for the
`dutchman` service); for builds of `main`, use `nightly`. SolidCraft's image takes a version
tag the same way, such as
`ghcr.io/tangency-labs/solidcraft-web:0.42.0`.
