# Grok Runtime

`grok-runtime` is a small, headless coding-agent binary. It talks directly to
OpenAI-compatible Responses APIs or Anthropic-compatible Messages APIs and does
not link the Grok pager, desktop UI, MCP, cloud storage, voice, update, or
telemetry stacks.

## Build

```sh
cargo build -p xai-grok-runtime-bin --profile runtime-release
```

The binary is written to `target/runtime-release/grok-runtime`.

The host needs `rg` (ripgrep) for the `list_files` and `search` tools. Shell
execution also uses the host shell and is only enabled with `--allow-shell`.

## Configure

Create `runtime.toml` for an OpenAI-compatible Responses provider:

```toml
backend = "responses"
base_url = "https://provider.example/v1"
model = "provider-model-id"
api_key_env = "PROVIDER_API_KEY"

# Mutating tools are opt-in.
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

Without `--allow-write`, only `read_file`, `list_files`, and `search` are
advertised. `--allow-write` adds `write_file` and exact-match `edit_file`;
`--allow-shell` adds command execution. File tools reject absolute paths,
`..`, and symlink escapes outside the configured workspace root.
