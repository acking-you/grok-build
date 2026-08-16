# Grok Runtime

`grok-runtime` is the original Grok Build agent exposed as an authenticated ACP
WebSocket service. The binary links `xai-grok-shell` directly and does not link
the pager/TUI crates. Its dependency features also leave UI-only clipboard,
crash reporting and CPU profiling, native cloud uploads, PDF/PPTX readers, and
raster-image decoding/compression out of this artifact. Normal Grok Build
binaries retain all of those optional capabilities.

This keeps the original session lifecycle, persistence and resume support,
automatic compaction, Todo/Task orchestration, subagents, web search, MCP, and
background shell-command handling. The runtime does not implement a second
agent loop.

## Build

```sh
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='-C force-unwind-tables=no -C llvm-args=-enable-machine-outliner=always -C llvm-args=-enable-merge-functions' \
  cargo build --profile runtime-release \
  -p xai-grok-runtime-bin \
  --bin grok-runtime

strip --strip-section-headers target/runtime-release/grok-runtime
```

The binary is written to `target/runtime-release/grok-runtime`. The extra flags
are stable-toolchain size optimizations for the native x86-64 Linux release;
omit them when building for another target. The final `strip` removes only the
ELF section table, which the Linux loader does not use, and keeps the packaged
artifact strictly below 50,000,000 bytes with the current dependency graph.

## Serve an Anthropic-compatible model

Keep the provider credential in the environment. For a gateway that accepts a
bearer token while speaking the Anthropic Messages protocol:

```sh
export GROK_RUNTIME_API_KEY='replace-me'

./target/runtime-release/grok-runtime \
  --backend anthropic \
  --base-url http://127.0.0.1:19182/api/kiro-gateway/v1 \
  --model claude-sonnet-5 \
  --auth-scheme bearer \
  --header anthropic-version=2023-06-01 \
  --yolo \
  --secret change-this-server-secret
```

Native Anthropic endpoints normally use `--auth-scheme x-api-key`, which is the
default for `--backend anthropic`.

## Serve an OpenAI Responses-compatible model

```sh
export GROK_RUNTIME_API_KEY='replace-me'

./target/runtime-release/grok-runtime \
  --backend responses \
  --base-url https://provider.example/v1 \
  --model provider-model-id \
  --auth-scheme bearer \
  --yolo \
  --secret change-this-server-secret
```

`GROK_RUNTIME_API_KEY` takes precedence. `--api-key-env NAME` selects another
environment variable without writing the credential to disk.

## Connect

The default endpoint is `ws://127.0.0.1:2419/ws`. Authenticate with either an
`Authorization: Bearer <server-secret>` header or the query parameter:

```text
ws://127.0.0.1:2419/ws?server-key=<server-secret>
```

The connection speaks the standard ACP JSON-RPC lifecycle: `initialize`, then
`session/new` or `session/load`, followed by `session/prompt`. Session updates
include the original detailed tool-call events. The process keeps its agent and
in-flight work alive across client reconnects.

`--yolo` is intended for trusted automation. Without it, the ACP client must
answer permission requests. Shell commands are executed by Grok Build's own
tool runtime and can access anything available to the OS user; deny rules and
hooks from the effective Grok configuration still apply.

## Native Grok configuration

`--config PATH` accepts a normal Grok Build TOML overlay and merges it over the
effective user configuration. Provider flags are only a convenience for one
runtime model; omit `--backend` to use models already declared in Grok config.

Equivalent model configuration looks like:

```toml
[models]
default = "gateway-claude"

[model.gateway-claude]
model = "claude-sonnet-5"
base_url = "http://127.0.0.1:19182/api/kiro-gateway/v1"
api_backend = "messages"
auth_scheme = "bearer"
env_key = "GROK_RUNTIME_API_KEY"
context_window = 200000
max_completion_tokens = 4096
inference_idle_timeout_secs = 300
supports_backend_search = true
extra_headers = { "anthropic-version" = "2023-06-01" }
```

Use `--disable-backend-search` to stop advertising provider-hosted search for
the configured model, or `--disable-web-search` to disable all Grok web-search
tools. `--inference-idle-timeout-secs` is a per-stream-chunk deadline: a stalled
provider request returns an error to the agent instead of blocking forever.
