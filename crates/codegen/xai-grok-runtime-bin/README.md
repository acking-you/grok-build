# Grok Runtime

`grok-runtime` is a small, headless coding-agent binary with hosted web search
and unrestricted shell execution. It talks directly to OpenAI-compatible
Responses APIs or Anthropic-compatible Messages APIs and does not link the Grok
pager, desktop UI, MCP, cloud storage, voice, update, or telemetry stacks.

## Build

```sh
cargo build -p xai-grok-runtime-bin --profile runtime-release
```

The binary is written to `target/runtime-release/grok-runtime`.

The host needs `rg` (ripgrep) for the `list_files` and `search` tools. Shell
execution uses the host shell and is enabled by default.

## Configure

Create `runtime.toml` for an OpenAI-compatible Responses provider:

```toml
backend = "responses"
base_url = "https://provider.example/v1"
model = "provider-model-id"
api_key_env = "PROVIDER_API_KEY"

# Structured file writes are opt-in. Shell execution is enabled by default.
allow_write = true
allow_shell = true
```

For an Anthropic-compatible Messages provider:

```toml
backend = "anthropic"
base_url = "https://provider.example/v1"
model = "provider-model-id"
api_key_env = "ANTHROPIC_API_KEY"
auth_scheme = "x_api_key"

[headers]
anthropic-version = "2023-06-01"
```

Secrets are read from the configured environment variable and are not stored in
the TOML file. `GROK_RUNTIME_API_KEY` can be used as a provider-independent
override. Gateways that expect bearer authentication can set
`auth_scheme = "bearer"`.

## Run

```sh
export PROVIDER_API_KEY='...'
target/runtime-release/grok-runtime \
  --config runtime.toml \
  --prompt 'Inspect this project and fix the failing test'
```

The agent keeps taking model/tool turns until the model returns a final response
with no tool calls. There is no turn limit by default. Set `--max-turns N` (or
`max_turns = N` in TOML) only when an explicit safety cap is desired.

Without `--allow-write`, only `read_file`, `list_files`, and `search` are
available alongside `web_search_runtime` and `bash`. `--allow-write` adds
`write_file` and exact-match `edit_file`. File tools reject absolute paths,
`..`, and symlink escapes outside the configured workspace root.

Shell commands run through the host shell, are not sandboxed, and may access or
modify anything allowed to the current OS user. Pass `--no-shell`, or set
`allow_shell = false` in TOML, to disable them.

`web_search_runtime` makes a dedicated provider-hosted search request and then
returns its result to the main agent. OpenAI Responses backends receive the
native `web_search` tool; Anthropic backends receive
`web_search_20250305`. The selected provider or gateway must support its hosted
web-search tool type.

Each tool invocation emits a detailed start line with its call id and concrete
arguments (workspace path, line range, query, command, or bounded edit/write
preview), followed by a completion line with status, elapsed time, output size,
and a bounded result preview.

Model requests emit matching `model:start`, `model:done`, or `model:error`
events so slow providers remain observable. Every inference and hosted-search
request has a 120-second total timeout by default; change it with
`--inference-timeout-secs` or `inference_timeout_secs` in TOML.

The default output cap is 8192 tokens for Responses and 4096 for Anthropic.
The lower Anthropic default avoids providers that stall before the first stream
event when asked for an 8192-token tool-use turn; either value can be overridden
with `--max-output-tokens` or `max_output_tokens` in TOML.
