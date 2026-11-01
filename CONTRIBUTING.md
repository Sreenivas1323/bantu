# Contributing to Bantu

Thanks for helping. Bantu is early (milestone M0), so the fastest way to help is to
try it, open issues, and pick up anything labelled `good first issue`.

## Setup

You need [Rust](https://rustup.rs) (stable), [Bun](https://bun.sh) and `git`.
macOS and Linux are supported; Windows comes later.

```sh
git clone https://github.com/Sreenivas1323/bantu && cd bantu
cargo run -p bantud                 # daemon on 127.0.0.1:47800
bun install && bun run dev:web      # web UI (Vite dev server)
```

`bantud --print-token` prints the local auth token the web UI asks for.

## Before you open a PR

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bun run typecheck && bun run build:web
```

CI runs the same checks on macOS and Ubuntu.

## Changing the protocol

Wire types live in `crates/protocol/src/lib.rs`. After changing them, regenerate the
TypeScript bindings and update `docs/PROTOCOL.md` in the same PR:

```sh
cargo test -p bantu-protocol    # writes packages/protocol/src/generated/*.ts
```

## Ground rules

- **Keep it free to run.** Nothing we add may require a server operated by the project.
  Cloud features deploy into the user's own account.
- **Copy features, never code.** Conductor is proprietary and Pane is AGPL-3.0. Read their
  docs and changelogs for ideas (see `docs/competitive/`), but write everything from scratch.
- **Small PRs.** One feature or fix per PR, with a short "why" in the description.

By contributing you agree your work is licensed under the Apache License 2.0.
