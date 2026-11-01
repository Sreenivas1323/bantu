# Bantu desktop

A thin Tauri 2 shell around the Bantu daemon. On launch it:

1. opens a window with a bundled splash page (`splash/index.html`);
2. checks `GET /v1/health` on `127.0.0.1:47800`. If the daemon isn't running, it starts `bantud` in its own process group, so agents keep running after the window closes;
3. reads the local token from `~/.bantu/local-token`;
4. points the window at the web UI the daemon serves, passing the token in the URL hash.

The shell holds no state of its own. Everything you see comes from the daemon over the same protocol the web and phone clients use.

## Develop

```sh
# from the repo root
cargo build -p bantud                # the shell finds target/debug/bantud in debug builds
bun install
bun run build:web                    # the daemon serves apps/web/dist
bun run dev:desktop                  # = tauri dev
```

To use the Vite dev server (hot reload) instead of the daemon-served build:

```sh
bun run dev:web                                        # terminal 1, proxies /v1 to the daemon
BANTU_DESKTOP_URL=http://localhost:5173/ bun run dev:desktop   # terminal 2
```

## Environment

| Variable | Default | Purpose |
|---|---|---|
| `BANTU_DAEMON_ADDR` | `127.0.0.1:47800` | Where to look for (and health-check) the daemon |
| `BANTU_DAEMON_BIN` | n/a | Explicit path to `bantud`. Otherwise: next to the app, then `PATH`, then `target/debug` in dev builds |
| `BANTU_HOME` | `~/.bantu` | Daemon data dir; the shell reads `local-token` and writes `logs/bantud.log` here |
| `BANTU_DESKTOP_URL` | `http://<addr>/` | UI to load (e.g. the Vite dev server) |
| `BANTU_WEB_DIR` | `apps/web/dist` in dev builds | Passed through to a daemon the shell starts |

If the shell starts the daemon itself, the daemon's output goes to `~/.bantu/logs/bantud.log`.

## Icons

`assets/icon.svg` is the source: a pawn, because *bantu* (బంటు) also means pawn. Regenerate with `bun run icons`. Only the desktop sizes are kept in `src-tauri/icons`.

## Build

`bun run build:desktop` produces a `.app`/`.dmg` (macOS) or `.deb`/`.AppImage` (Linux). Bundling `bantud` as a sidecar and code signing are not wired up yet. For now, install `bantud` on `PATH`.
