# Use Bantu from other devices

`bantud` listens on `127.0.0.1` only. To reach it from your phone, another laptop, or a
browser anywhere, put an authenticated tunnel in front of it. Bantu runs no relay, so
nothing passes through servers of ours; your agents and code stay on your machine.

Every request still needs the daemon's token (`bantud url` prints a link that carries it
in the `#fragment`, which browsers never send to servers).

## Tailscale (recommended, free for personal use)

Tailscale gives each machine a private HTTPS name that only your devices can reach.

On the machine running `bantud`:

```sh
tailscale serve --bg 47800
# → https://<machine>.<tailnet>.ts.net
BANTU_PUBLIC_URL=https://<machine>.<tailnet>.ts.net bantud service install
bantud url --qr
```

`service install` remembers `BANTU_PUBLIC_URL`, which also tells the daemon to accept
requests for that hostname. Install Tailscale on your phone, sign in, and scan the QR
code. Add the page to your home screen for an app-like window.

## Cloudflare Tunnel (free, needs a domain on Cloudflare)

```sh
cloudflared tunnel --url http://127.0.0.1:47800 --hostname bantu.example.com
BANTU_PUBLIC_URL=https://bantu.example.com bantud service install
```

Protect the hostname with Cloudflare Access (email login) as a second lock in front of
the token. [CLOUDFLARE.md](CLOUDFLARE.md) has a script that sets up the tunnel, both
hostnames and Access in one go, and has the daemon check the Access sign-in too.

## SSH (no extra software)

From the client machine:

```sh
ssh -N -L 47800:127.0.0.1:47800 you@server
```

Then open `http://127.0.0.1:47800/#token=…` locally. No host settings are needed because
the request arrives as `127.0.0.1`.

## Settings

| Variable / flag | Meaning |
|---|---|
| `BANTU_PUBLIC_URL` / `--public-url` | How other devices reach this daemon. Printed by `bantud url`; its host is allowed. |
| `BANTU_ALLOWED_HOSTS` / `--allowed-host` | More `Host` names to accept (comma-separated). |
| `--allow-remote` | Bind non-loopback addresses and accept any `Host`. Only behind a trusted proxy. |

Requests for any other `Host` get `403`, which blocks DNS-rebinding attacks from web pages.

## Install it as an app

The web client is a PWA. On a phone, open your daemon's HTTPS address and use **Add to Home
Screen**; on desktop Chrome or Edge, use the install icon in the address bar. It opens in
its own window, and the workspace list becomes a drawer on small screens. Installing needs
HTTPS, which Tailscale Serve and Cloudflare Tunnel both provide.

## Port forwarding

The **Preview** tab lists servers listening on the workspace's reserved ports, with a link to
open each one. Start dev servers on `$BANTU_PORT` so they show up.

Pages, assets and WebSockets under the preview's path are all forwarded, and root-absolute
assets like `/assets/app.js` find their way back. The exception is a WebSocket opened at a root
path, which can't be redirected, and that's where many hot reload clients connect. Terminals in
the workspace have `$BANTU_PREVIEW_BASE` set for this, so point your dev server's base path at
it when you need hot reload through a preview, e.g. `vite --port $BANTU_PORT --base $BANTU_PREVIEW_BASE`.

Previews use a second port (`47801` by default), so that a previewed page can't reach your
daemon's token. On the daemon's own machine there's nothing to set up. For other devices,
expose that port as well and tell the daemon its address:

```sh
tailscale serve --bg --https=8443 47801
BANTU_PREVIEW_URL=https://<machine>.<tailnet>.ts.net:8443 bantud service install
```

With Cloudflare Tunnel, give it its own hostname, e.g. `preview.example.com`. Links last 5
minutes, and each one opens a single port of a single workspace.

## Alerts on your phone

Choose **Turn on alerts** in the status bar. The daemon then alerts that device when an agent
needs approval, asks a question or finishes, even with Bantu closed. Alerts travel through
the browser's own push service, encrypted so that only your device can read them. There's no
server of ours in the path. On iPhone and iPad, add Bantu to your home screen first; Safari
only offers Web Push to installed apps. The daemon needs outbound HTTPS and `curl`.

## Host the web client yourself

`bantud` already serves the client, so this is optional. It's useful if you want one
address for several daemons, or to keep the client up while a machine is stopped. The client is
plain static files, and nothing about your daemon or token reaches the host:

```sh
bun install && bun run build:web    # output in apps/web/dist
```

Upload `apps/web/dist` to any static host (GitHub Pages, Vercel, Cloudflare Pages, S3), in any
folder. Then pair a device with a link that names both:

```sh
BANTU_APP_URL=https://bantu.example.com bantud url --qr
# → https://bantu.example.com/#daemon=https://box.tail1234.ts.net&token=…
```

The daemon needs an HTTPS address, because browsers block `http://` requests from an HTTPS
page. The exception is a daemon on the same machine at `http://127.0.0.1:47800`, though
some browsers ask for permission to reach local addresses from a public site.
