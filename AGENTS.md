# Repository guide for AI agents

Operational guidance for AI coding agents working in this repository. Keep it
short and durable; see [`README.md`](README.md) for the full project overview and
[`crates/codegen/xai-grok-runtime-bin/README.md`](crates/codegen/xai-grok-runtime-bin/README.md)
for the runtime.

`CLAUDE.md` is a symlink to this file.

## What this repo is

- A **fork** of SpaceXAI's Grok Build (the `grok` / `xai-grok-pager` TUI). The
  fork adds **`grok-runtime`** (`crates/codegen/xai-grok-runtime-bin`): the same
  agent runtime served headlessly over an ACP WebSocket, runnable on the OpenAI
  Responses API, the Anthropic Messages API, or native Grok login credentials.
- **Branch model:** `main` is the slimmed working branch — base all work on it.
  `upstream` is a pristine mirror of the public tree, merged into `main`
  periodically; **do not commit onto `upstream`**.

## Build / test / lint

- The Rust toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml)
  (rustup installs it automatically).
- **Building needs a working `protoc` for proto codegen.** The build resolves
  [`bin/protoc`](bin/protoc) via DotSlash, so have `dotslash` on `PATH`
  (`cargo install dotslash`), or provide a `protoc` on `PATH` / `$PROTOC`.
- **Always target a specific crate — full-workspace builds are slow.**

  ```sh
  cargo check  -p <crate>
  cargo test   -p <crate>
  cargo clippy -p <crate>   # lint config: clippy.toml at repo root
  cargo fmt --all           # rustfmt.toml at repo root
  ```

- Slim runtime build:

  ```sh
  cargo build --profile runtime-release -p xai-grok-runtime-bin --bin grok-runtime
  ```

## Conventions & gotchas

- The **root `Cargo.toml` is generated** (workspace members, dependency
  versions, lints, profiles) — treat it as read-only and edit per-crate
  `Cargo.toml` files instead.
- The `dev` profile sets `panic = "abort"`; tests still run because Cargo forces
  unwinding for the `test`/`bench` profiles.
- Do not edit vendored sources under [`third_party/`](third_party/) or generated
  proto output.
- Keep upstream (`grok` TUI) behavior intact. Runtime slimming is done via Cargo
  features (`default-features = false`) plus stubs — **not** by forking the agent
  loop. Fixes that must touch shared crates (e.g. `xai-grok-sampler`) affect the
  TUI too; call that out.
- This repository does not accept external pull requests or unsolicited patches
  (see [`CONTRIBUTING.md`](CONTRIBUTING.md)).

## Layout

See the Repository layout table in [`README.md`](README.md#repository-layout).
