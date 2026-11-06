# Bantu behind Cloudflare

Reach the daemon on your own machine from any device at your own domain, with a
Cloudflare sign-in in front of it. It's free: Tunnel and Access cost nothing for
up to 50 users. You need a domain whose DNS is on Cloudflare.

For bursts of parallel tasks on Cloudflare Containers, see
[CLOUDFLARE-BURST.md](CLOUDFLARE-BURST.md).

## Set it up

```sh
brew install cloudflared            # or your distribution's package

export CLOUDFLARE_ACCOUNT_ID=…      # Dashboard → any domain → Overview
export CLOUDFLARE_API_TOKEN=…       # "Access: Apps and Policies Write" + "Access: Organizations Read"

deploy/cloudflare/setup.sh \
  --app bantu.example.com --preview preview.bantu.example.com \
  --email you@example.com
```

The script:

1. Creates an Access policy that lets in only the emails you give, and an Access
   application for each hostname. This happens before any DNS exists, so the
   addresses are never reachable without signing in.
2. Creates a tunnel named `bantu` (`--tunnel` to change it), points both hostnames
   at it, and writes its config: the app goes to port 47800, previews to 47801.
   An existing DNS record is never overwritten.
3. Installs the tunnel as a service if you had no `~/.cloudflared/config.yml`.
   Otherwise it writes `bantu-<tunnel>.yml` beside it and prints how to run it.
4. Prints the line that reinstalls `bantud` with its public addresses and Access
   settings.

Then pair a phone with `bantud url --qr`. The first visit asks you to sign in with
Cloudflare, then the pairing link does the rest.

## How it's protected

- **Two locks.** Cloudflare asks for your sign-in before a request reaches your
  machine, and the daemon still needs its own token.
- **The daemon checks Access too.** With `BANTU_CF_ACCESS_TEAM` and
  `BANTU_CF_ACCESS_AUD` set, any request for a public hostname must carry a valid
  Access token (`Cf-Access-Jwt-Assertion`), signed by your team for one of your
  two applications and not expired. So a mistake in the Access setup, like a
  deleted policy, fails closed. Requests to `127.0.0.1` from the machine itself
  don't need one.
- **Previews have their own hostname,** so a previewed page is on a different
  origin from the daemon's token.
- **No open ports.** The tunnel connects out to Cloudflare; nothing listens on
  your network.

When a sign-in expires, the app shows "Sign in again" instead of failing quietly.
Open the app from the daemon's own address: a copy hosted elsewhere can't carry the
Access sign-in.

## Doing it by hand

If you'd rather not give the script an API token, create the two Access
applications (self-hosted, one per hostname) in Zero Trust → Access →
Applications, copy each one's **Application Audience (AUD) tag**, and run
`deploy/cloudflare/setup.sh … --no-access`. Then add
`BANTU_CF_ACCESS_TEAM=<team>.cloudflareaccess.com BANTU_CF_ACCESS_AUD=<app aud>,<preview aud>`
to the printed `bantud service install` line.

> Status: the daemon's Access check is tested end to end with locally signed
> tokens, and the script was run against a fake Cloudflare API and a stand-in
> `cloudflared`. It hasn't been run against a real Cloudflare account yet.
