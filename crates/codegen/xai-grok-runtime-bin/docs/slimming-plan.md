# `grok-runtime` positioning & slimming plan

This document captures where the `grok-runtime` binary sits in this fork, what
it can already do, and a concrete staged plan to make it *relatively small*
without forking a second agent loop.

## Fork positioning

Upstream **Grok Build** ships one product: the `grok` / `xai-grok-pager` TUI, a
full-screen terminal AI coding agent. This fork's addition is **`grok-runtime`**
(`crates/codegen/xai-grok-runtime-bin`): the *same* agent runtime
(`xai_grok_shell::agent::MvpAgent`) exposed headlessly as an authenticated **ACP
WebSocket service**, with **no TUI/pager crates linked**.

Design trajectory (fork commits): a custom minimal loop (`70173fd`) was replaced
by reusing the real agent behind a slim serve wrapper (`93b3c86`). So
`grok-runtime` keeps full fidelity (sessions, resume, compaction, todos,
subagents, web search, MCP, background shell) and slims **only** via Cargo
features + the `runtime-release` profile — never by reimplementing the agent.

## Three inference backends (all working today)

The runtime CLI (`src/serve.rs`) already supports all three targets the fork is
meant to run on. Verified on the current tree (`--backend responses` validates
flags; `--backend anthropic` starts and serves; bare mode loads native config):

| Backend | CLI | Wire | Credential source | Auth header |
| --- | --- | --- | --- | --- |
| OpenAI Responses | `--backend responses --base-url … --model …` | `api_backend = "responses"` | `GROK_RUNTIME_API_KEY` → `--api-key-env NAME` (BYOK) | `Authorization: Bearer` (default) |
| Anthropic Messages | `--backend anthropic --base-url … --model …` | `api_backend = "messages"` | `GROK_RUNTIME_API_KEY` → `--api-key-env NAME` (BYOK) | `x-api-key` (default), or `--auth-scheme bearer` |
| Native Grok login | *(no `--backend`)* | `responses` @ `cli-chat-proxy.grok.com/v1`, default model `grok-4.6` | `~/.grok/auth.json` (OAuth / `grok login --device-code`) with live bearer refresh, or `XAI_API_KEY` | `Authorization: Bearer` (refreshed per request) |

Auth resolution lives in `xai_grok_shell::agent::config::resolve_credentials`
(BYOK `api_key`/`env_key` wins first; else `AuthManager` session token; else
`XAI_API_KEY`). BYOK never uses the live bearer resolver; native mode wires
`WireValidBearerResolver` via `SessionTokenAuthGate`.

## Size baseline (x86_64-linux, this VM)

| Build | Size | Notes |
| --- | --- | --- |
| `cargo build -p xai-grok-runtime-bin` (dev) | ~498 MB | almost entirely debug symbols; not representative |
| `cargo build --profile runtime-release -p xai-grok-runtime-bin` | **~55.8 MB** | stripped, `opt-level="z"`, fat LTO — the real "slim" number |

`runtime-release` is already the shipping profile. Further shrinking means
**removing dependencies from the serve build**, since the code itself is small.

## What is already trimmed

`grok-runtime` links `xai-grok-shell` with `default-features = false, features =
["serve-runtime"]`, which drops these from the artifact (stubbed at the call
sites): PDF/PPTX readers, native cloud uploads (`gcloud-storage`), clipboard
(`arboard`/`wl-clipboard`), raster image codecs (`image`), CPU profiling
(`pprof`), and OpenTelemetry exporters.

**Gap:** `serve-runtime` is currently an *empty marker* feature — it gates no
`#[cfg]` code. The heavy, non-optional dependencies below are still linked
wholesale through `xai-grok-shell`.

## Slimming targets (heavy, currently non-optional)

Ranked by size-win / risk. "On serve path?" = reached from
`run_agent_server` → `MvpAgent` for a normal prompt+tools turn.

| # | Subsystem / dep | Size class | On serve path? | Capability dropped if gated off | Risk |
| --- | --- | --- | --- | --- | --- |
| 1 | `xai-grok-session-search` (`rusqlite` bundled SQLite) | Large (C lib) | No (session FTS only) | `session/search` FTS over past sessions | Med (types in storage/persistence signatures; `Off` state already exists) |
| 2 | `xai-grok-plugin-marketplace` + bundle sync | Medium | No (extension-only) | plugin marketplace install/sync | Low |
| 3 | `nucleo` + `extensions/suggest` | Medium | No (interactive suggest) | fuzzy slash/file suggestions for clients | Low |
| 4 | `xai-codebase-graph` + `code_nav` | Medium | No (lazy/feature) | code navigation / codebase indexing | Med |
| 5 | `notify` / `notify-debouncer-mini` / `xai-fsnotify` | Small–Med | Partial (config watch, fs watch) | config hot-reload, live fs watch | Med |
| 6 | `webbrowser` + OAuth browser-open | Small | No (headless can't open a browser) | browser OAuth (device-code still works) | Low |
| 7 | `xai-fast-worktree` + extra `git2` worktree use | Medium | No unless `--worktree` | session worktrees / auto-GC | Med |
| 8 | `git2` (`vendored-libgit2`) core | **Large (C lib)** | Partial (project root, skills, trust, trace) | git-aware project root / skills / worktrees | **High** |
| 9 | `xai-grok-mcp` (`rmcp`, private `reqwest 0.13`) | **Large** | Only if client sends `mcpServers` | MCP tool servers | **High** |

Genuinely required for a "full agent" serve and **not** slimming candidates:
terminal/PTY stack (`portable-pty`, `process-wrap`, sandbox), the sampler + auth
+ session persistence, and subagents.

## Proposed approach

Turn `serve-runtime` (and/or a small set of new **default-on** features on
`xai-grok-shell`) into *real* gates so the default `grok` TUI keeps every
capability while `grok-runtime` (default-features off) drops the selected
subsystems. Each gated dependency becomes `optional = true`; its module + call
sites get `#[cfg(feature = …)]`, reusing existing "off" states
(e.g. `SearchIndex::Off`) where present.

### Two tiers (pick one to steer the cuts)

- **Tier A — full-agent fidelity (recommended default).** Gate off only stages
  1–6 (session FTS, marketplace, suggest/nucleo, codebase-graph, fs-watch,
  browser-open). Keeps git-aware features, worktrees, and MCP. Lower risk;
  moderate size win.
- **Tier B — minimal BYOK automation.** Additionally gate stages 7–9
  (worktrees, `git2` core, MCP). Largest size win, but the runtime loses
  git-aware project discovery/skills/worktrees and MCP tool servers.

### Staged execution (each stage: build both `grok` and `grok-runtime`, then measure `runtime-release` size delta)

1. Make `serve-runtime` a real feature; gate stage 1 (`xai-grok-session-search`). Verify both binaries build; record size delta.
2. Stages 2–3 (marketplace/bundle, suggest/nucleo).
3. Stages 4–6 (codebase-graph, fs-watch, browser-open).
4. *(Tier B only)* Stages 7–9 (worktrees, `git2`, MCP) behind their own opt-in-to-drop features.

Non-negotiable invariant: the default-feature `grok` / `xai-grok-pager` build
must remain byte-for-byte functional; only the `default-features = false`
runtime build changes.
