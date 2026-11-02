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
the token.

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
