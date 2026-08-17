<div align="center">

<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://media.x.ai/v1/website/spacexai-symbol-white-transparent-0c31957f.png">
    <source media="(prefers-color-scheme: light)" srcset="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png">
    <img alt="SpaceXAI logo" src="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png" width="96">
  </picture>
  <br>
  Grok Build (<code>grok</code>) — slim runtime fork
</h1>

**Grok Build** is SpaceXAI's terminal-based AI coding agent. **This repository is
a fork** that keeps the upstream `grok` TUI intact and adds a small, standalone
**`grok-runtime`** — the same agent runtime exposed headlessly as an ACP
WebSocket service — that can run on the **OpenAI Responses API**, the **Anthropic
Messages API**, or **Grok's own login credentials**.

[About this fork](#about-this-fork) ·
[Branch model](#branch-model) ·
[`grok-runtime`](#grok-runtime--slim-headless-runtime) ·
[Building](#building-from-source) ·
[Repository layout](#repository-layout) ·
[License](#license)

![Grok Build TUI](https://media.x.ai/v1/website/universe-tui-screenshot-6f7a0837.png)

**Learn more about upstream Grok Build at [x.ai/cli](https://x.ai/cli)**

</div>

---

## About this fork

Upstream Grok Build ships one product: the full-screen `grok` / `xai-grok-pager`
TUI, periodically synced from the SpaceXAI monorepo. This fork's goal is to turn
that same agent into something you can **run as a small headless service against
your own model provider**, without reimplementing the agent loop.

Concretely, the fork:

- **Adds `grok-runtime`** ([`crates/codegen/xai-grok-runtime-bin`](crates/codegen/xai-grok-runtime-bin)):
  the original `xai_grok_shell::agent::MvpAgent` served over an authenticated ACP
  WebSocket. It links `xai-grok-shell` directly and **does not link the
  pager/TUI crates**. It is not a second agent loop — you keep the original
  session lifecycle, resume, automatic compaction, Todo/Task orchestration,
  subagents, web search, MCP, and background shell handling.
- **Runs on three inference backends** (see the table below): OpenAI Responses,
  Anthropic Messages, and native Grok login.
- **Trims the runtime artifact.** `grok-runtime` builds `xai-grok-shell` /
  `xai-grok-tools` with `default-features = false`, dropping capabilities a
  headless service does not need — PDF/PPTX readers, native cloud uploads,
  clipboard, raster image codecs, CPU profiling, and OpenTelemetry exporters
  (replaced with stubs). The default `grok` TUI keeps all of these.
- **Adds a portable `runtime-release` build profile** (stripped, `opt-level="z"`,
  fat LTO) that produces a self-contained binary of roughly **~56 MB** on
  x86_64-linux.
- **Tunes headless behavior**: runs the agent to completion by default when
  serving, exposes hosted web search to the runtime, hardens inference
  idle-timeouts, and fixes an Anthropic large-output stall.

The upstream `grok` TUI stays fully functional. Beyond adding the runtime and
narrowing its dependency set, a few hardening fixes also land in **shared**
crates — notably `xai-grok-sampler` inference idle-timeout handling (the wait
for response headers / stream start is now bounded) and an Anthropic
large-output stall fix. The TUI uses that same inference path, so it is affected
by those shared changes too.

## Branch model

| Branch | Role |
|--------|------|
| **`main`** | The slimmed fork — the working branch that carries the `grok-runtime` work on top of upstream. Build and run from here. |
| **`upstream`** | A pristine mirror of the public SpaceXAI Grok Build tree (only `Synced from monorepo` commits). It is **merged into `main` periodically** to pull in upstream changes. |

The root [`SOURCE_REV`](SOURCE_REV) file records the full monorepo commit SHA the
`upstream` mirror was synced from.

> [!NOTE]
> This is a fork for local/self-hosted use. Like upstream, this repository does
> not accept external pull requests or unsolicited patches — see
> [`CONTRIBUTING.md`](CONTRIBUTING.md).

## `grok-runtime` — slim headless runtime

`grok-runtime` serves the agent over `ws://<bind>/ws` (default
`127.0.0.1:2419`). Clients speak the standard ACP JSON-RPC lifecycle
(`initialize` → `session/new` / `session/load` → `session/prompt`) and
authenticate with the server secret. It supports three ways to reach a model:

| Backend | Invocation | Wire protocol | Credentials |
|---------|------------|---------------|-------------|
| **OpenAI Responses** | `--backend responses --base-url … --model …` | `responses` | `GROK_RUNTIME_API_KEY` (or `--api-key-env NAME`); `Authorization: Bearer` |
| **Anthropic Messages** | `--backend anthropic --base-url … --model …` | `messages` | `GROK_RUNTIME_API_KEY` (or `--api-key-env NAME`); `x-api-key` by default, or `--auth-scheme bearer` |
| **Native Grok login** | *(omit `--backend`)* | `responses` @ `cli-chat-proxy.grok.com/v1`, default model `grok-4.6` | `~/.grok/auth.json` from `grok login` (OAuth / `--device-code`), refreshed automatically; or `XAI_API_KEY` |

Build and run:

```sh
# Build the slim runtime (writes target/runtime-release/grok-runtime)
cargo build --profile runtime-release -p xai-grok-runtime-bin --bin grok-runtime

# Example: serve an Anthropic-compatible gateway (BYOK).
# --backend anthropic defaults to the x-api-key scheme; the Anthropic Messages
# API also requires the anthropic-version header, which the runtime forwards
# only when you pass it explicitly via --header.
export GROK_RUNTIME_API_KEY='replace-me'
./target/runtime-release/grok-runtime \
  --backend anthropic \
  --base-url https://provider.example/v1 \
  --model claude-sonnet-5 \
  --header anthropic-version=2023-06-01 \
  --secret change-this-server-secret

# Example: use your existing Grok login (no --backend)
grok login            # once, stores ~/.grok/auth.json
./target/runtime-release/grok-runtime --secret change-this-server-secret
```

See [`crates/codegen/xai-grok-runtime-bin/README.md`](crates/codegen/xai-grok-runtime-bin/README.md)
for the full CLI, connection details, `--config` overlays, `--yolo` automation
notes, and equivalent native-config TOML.

## Installing the released binary

The upstream `grok` TUI (built from this tree as `xai-grok-pager`, shipped
officially as `grok`) is preserved; aside from the shared inference-hardening
fixes noted above, it behaves as upstream. Prebuilt official binaries are
published for macOS, Linux, and Windows:

```sh
curl -fsSL https://x.ai/cli/install.sh | bash   # macOS / Linux / Git Bash
irm https://x.ai/cli/install.ps1 | iex          # Windows PowerShell
grok --version
```

See the [changelog](https://x.ai/build/changelog) for upstream releases.

## Building from source

Requirements:

- **Rust** — the toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` installs it automatically on first build.
- **[DotSlash](https://dotslash-cli.com)** — required so hermetic tools under
  [`bin/`](bin/) (notably [`bin/protoc`](bin/protoc)) can download and run.
  Install it and ensure `dotslash` is on your `PATH` **before** building:

  ```sh
  cargo install dotslash
  # or: prebuilt packages — https://dotslash-cli.com/docs/installation/
  /usr/bin/env dotslash --help   # sanity check
  ```

- **protoc** — proto codegen resolves [`bin/protoc`](bin/protoc) via DotSlash,
  or falls back to a `protoc` on `PATH` / `$PROTOC`.
- macOS and Linux are supported build hosts; Windows builds are best-effort
  and not currently tested from this tree.

```sh
cargo run -p xai-grok-pager-bin              # build + launch the TUI
cargo build -p xai-grok-pager-bin --release  # release binary: target/release/xai-grok-pager
cargo check -p xai-grok-pager-bin            # fast validation

cargo build --profile runtime-release -p xai-grok-runtime-bin  # the slim runtime
```

The TUI binary artifact is named `xai-grok-pager`; official installs ship it as
`grok`. On first launch it opens your browser to authenticate — see the
[authentication guide](crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md).

## Documentation

Full online documentation for upstream Grok Build is available at
[docs.x.ai/build/overview](https://docs.x.ai/build/overview).

The user guide ships with the pager crate:
[`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/)
— getting started, keyboard shortcuts, slash commands, configuration, theming,
MCP servers, skills, plugins, hooks, headless mode, sandboxing, and more.

Fork-specific runtime docs live in
[`crates/codegen/xai-grok-runtime-bin/README.md`](crates/codegen/xai-grok-runtime-bin/README.md).

## Repository layout

| Path | Contents |
|------|----------|
| `crates/codegen/xai-grok-runtime-bin` | **Fork addition** — the slim `grok-runtime` ACP serve binary |
| `crates/codegen/xai-grok-pager-bin` | Composition-root package; builds the `xai-grok-pager` binary |
| `crates/codegen/xai-grok-pager` | The TUI: scrollback, prompt, modals, rendering |
| `crates/codegen/xai-grok-shell` | Agent runtime + leader/stdio/headless entry points |
| `crates/codegen/xai-grok-tools` | Tool implementations (terminal, file edit, search, ...) |
| `crates/codegen/xai-grok-workspace` | Host filesystem, VCS, execution, checkpoints |
| `crates/codegen/...` | The rest of the CLI crate closure (config, MCP, markdown, sandbox, ...) |
| `crates/common/`, `crates/build/`, `prod/mc/` | Small shared leaf crates pulled in by the closure |
| `third_party/` | Vendored upstream source (Mermaid diagram stack) — see below |

> [!IMPORTANT]
> The root `Cargo.toml` (workspace members, dependency versions, lints,
> profiles) is **generated** — treat it as read-only. Prefer editing per-crate
> `Cargo.toml` files.

## Development

```sh
cargo check -p <crate>        # always target specific crates; full-workspace builds are slow
cargo test -p xai-grok-config # per-crate tests
cargo clippy -p <crate>       # lint config: clippy.toml at the repo root
cargo fmt --all               # rustfmt.toml at the repo root
```

## Contributing

> [!NOTE]
> This repository does not accept external pull requests or unsolicited patches.
> See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

First-party code in this repository is licensed under the **Apache License,
Version 2.0** — see [`LICENSE`](LICENSE).

Third-party and vendored code remains under its original licenses. See:

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) — crates.io / git dependencies,
  bundled UI themes, and **in-tree source ports** (including openai/codex and
  sst/opencode tool implementations)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md)
  — crate-local notice for the codex and opencode ports (license texts +
  Apache §4(b) change notice)
- [`third_party/NOTICE`](third_party/NOTICE) — vendored Mermaid-stack index
