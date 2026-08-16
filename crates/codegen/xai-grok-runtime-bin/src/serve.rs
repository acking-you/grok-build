use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use toml::Value;
use xai_grok_shell::agent::config::{AgentMode, Config as AgentConfig, RuntimeResolutionContext};

const RUNTIME_MODEL_ID: &str = "grok-runtime";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Backend {
    Responses,
    #[value(alias = "messages")]
    Anthropic,
}

impl Backend {
    fn api_backend(self) -> &'static str {
        match self {
            Self::Responses => "responses",
            Self::Anthropic => "messages",
        }
    }

    fn default_key_env(self) -> &'static str {
        match self {
            Self::Responses => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
        }
    }

    fn default_auth_scheme(self) -> AuthScheme {
        match self {
            Self::Responses => AuthScheme::Bearer,
            Self::Anthropic => AuthScheme::XApiKey,
        }
    }

    fn default_max_output_tokens(self) -> u32 {
        match self {
            Self::Responses => 8_192,
            Self::Anthropic => 4_096,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum AuthScheme {
    Bearer,
    #[value(name = "x-api-key", alias = "x_api_key")]
    XApiKey,
}

impl AuthScheme {
    fn config_value(self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::XApiKey => "x_api_key",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "grok-runtime",
    about = "Serve the original Grok Build agent runtime without the TUI"
)]
pub struct Cli {
    /// Address for the ACP WebSocket server.
    #[arg(long, default_value = "127.0.0.1:2419")]
    pub bind: SocketAddr,

    /// WebSocket bearer secret. Generated when omitted.
    #[arg(long, env = "GROK_AGENT_SECRET")]
    pub secret: Option<String>,

    /// Optional Grok-format TOML overlay, merged over the normal effective config.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Configure an OpenAI Responses or Anthropic Messages model for this server.
    #[arg(long, value_enum)]
    pub backend: Option<Backend>,

    /// Third-party inference base URL, normally ending in /v1.
    #[arg(long, env = "GROK_RUNTIME_BASE_URL")]
    pub base_url: Option<String>,

    /// Provider model slug sent on inference requests.
    #[arg(long, env = "GROK_RUNTIME_MODEL")]
    pub model: Option<String>,

    /// Environment variable containing the provider API key.
    #[arg(long, env = "GROK_RUNTIME_API_KEY_ENV")]
    pub api_key_env: Option<String>,

    /// Provider authentication header style.
    #[arg(long, value_enum)]
    pub auth_scheme: Option<AuthScheme>,

    /// Literal provider header in KEY=VALUE form. Repeatable.
    #[arg(long = "header", value_parser = parse_key_value)]
    pub headers: Vec<(String, String)>,

    /// Secret provider header in KEY=ENV_VAR form. Repeatable.
    #[arg(long = "header-env", value_parser = parse_key_value)]
    pub env_headers: Vec<(String, String)>,

    /// Model context window used by the original auto-compaction policy (default: 200000).
    #[arg(long)]
    pub context_window: Option<u64>,

    /// Maximum completion tokens requested from the provider.
    #[arg(long)]
    pub max_output_tokens: Option<u32>,

    /// Maximum idle time between inference stream chunks (default: 300).
    #[arg(long)]
    pub inference_idle_timeout_secs: Option<u64>,

    /// Disable provider-hosted web search for the configured model.
    #[arg(long)]
    pub disable_backend_search: bool,

    /// Disable all web-search tools in Grok Build.
    #[arg(long)]
    pub disable_web_search: bool,

    /// Bypass tool permission prompts for sessions served by this process.
    #[arg(long)]
    pub yolo: bool,
}

pub struct ServeRuntime {
    pub bind: SocketAddr,
    pub secret: String,
    pub agent_config: AgentConfig,
}

impl ServeRuntime {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        if cli.backend.is_none()
            && (cli.base_url.is_some()
                || cli.model.is_some()
                || cli.api_key_env.is_some()
                || cli.auth_scheme.is_some()
                || !cli.headers.is_empty()
                || !cli.env_headers.is_empty()
                || cli.context_window.is_some()
                || cli.max_output_tokens.is_some()
                || cli.inference_idle_timeout_secs.is_some()
                || cli.disable_backend_search)
        {
            bail!("--backend is required when provider options are set");
        }

        let mut raw_config = xai_grok_shell::config::load_effective_config()
            .context("load Grok Build effective configuration")?;
        if let Some(path) = cli.config.as_ref() {
            let text = fs::read_to_string(path)
                .with_context(|| format!("read config overlay {}", path.display()))?;
            let overlay: Value = toml::from_str(&text)
                .with_context(|| format!("parse config overlay {}", path.display()))?;
            xai_grok_shell::config::deep_merge_toml(&mut raw_config, &overlay);
        }

        if let Some(backend) = cli.backend {
            let context_window = cli.context_window.unwrap_or(200_000);
            if context_window == 0 {
                bail!("context_window must be greater than zero");
            }
            let context_window = i64::try_from(context_window)
                .context("context_window exceeds the supported integer range")?;
            let inference_idle_timeout_secs = cli.inference_idle_timeout_secs.unwrap_or(300);
            if inference_idle_timeout_secs == 0 {
                bail!("inference_idle_timeout_secs must be greater than zero");
            }
            let inference_idle_timeout_secs = i64::try_from(inference_idle_timeout_secs)
                .context("inference_idle_timeout_secs exceeds the supported integer range")?;
            let base_url = required("base_url", cli.base_url)?;
            let model = required("model", cli.model)?;
            let auth_scheme = cli
                .auth_scheme
                .unwrap_or_else(|| backend.default_auth_scheme());
            let key_env = cli
                .api_key_env
                .unwrap_or_else(|| backend.default_key_env().to_owned());
            let max_output_tokens = cli
                .max_output_tokens
                .unwrap_or_else(|| backend.default_max_output_tokens());
            if max_output_tokens == 0 {
                bail!("max_output_tokens must be greater than zero");
            }
            let patch = provider_patch(
                backend,
                &base_url,
                &model,
                &key_env,
                auth_scheme,
                context_window,
                max_output_tokens,
                inference_idle_timeout_secs,
                !cli.disable_backend_search,
                cli.headers,
                cli.env_headers,
            );
            xai_grok_shell::config::deep_merge_toml(&mut raw_config, &patch);
        }

        let mut agent_config = AgentConfig::new_from_toml_cfg(&raw_config)
            .map_err(anyhow::Error::msg)
            .context("build Grok Build agent configuration")?;
        if cli.backend.is_some() {
            agent_config.default_model_override = Some(RUNTIME_MODEL_ID.to_owned());
        }
        agent_config.default_yolo_mode = cli.yolo;
        agent_config.mode = AgentMode::Serve;
        agent_config.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw_config,
            remote_settings: None,
            is_headless: true,
            cli_subagents: None,
            cli_web_search_model: None,
            cli_session_summary_model: None,
            memory_enabled_override: None,
            disable_web_search: cli.disable_web_search,
            todo_gate: false,
            laziness_debug_log: None,
            storage_mode: None,
        });

        Ok(Self {
            bind: cli.bind,
            secret: cli.secret.unwrap_or_else(generate_secret),
            agent_config,
        })
    }

    pub async fn run(self) -> Result<()> {
        xai_grok_shell::agent::app::suppress_otel();
        xai_grok_shell::agent::mvp_agent::warm_async_http_client();
        let server_config = xai_grok_shell::agent::ServerConfig {
            bind_addr: self.bind,
            secret: self.secret.clone(),
        };
        eprintln!(
            "Grok Build runtime server listening on ws://{}/ws",
            self.bind
        );
        eprintln!("server secret: {}", self.secret);
        xai_grok_shell::agent::run_agent_server(server_config, self.agent_config).await
    }
}

#[allow(clippy::too_many_arguments)]
fn provider_patch(
    backend: Backend,
    base_url: &str,
    model: &str,
    key_env: &str,
    auth_scheme: AuthScheme,
    context_window: i64,
    max_output_tokens: u32,
    inference_idle_timeout_secs: i64,
    supports_backend_search: bool,
    headers: Vec<(String, String)>,
    env_headers: Vec<(String, String)>,
) -> Value {
    let mut entry = toml::map::Map::new();
    entry.insert("model".into(), Value::String(model.to_owned()));
    entry.insert("base_url".into(), Value::String(base_url.to_owned()));
    entry.insert(
        "api_backend".into(),
        Value::String(backend.api_backend().to_owned()),
    );
    entry.insert(
        "auth_scheme".into(),
        Value::String(auth_scheme.config_value().to_owned()),
    );
    entry.insert(
        "env_key".into(),
        Value::Array(
            ["GROK_RUNTIME_API_KEY", key_env]
                .into_iter()
                .map(|name| Value::String(name.to_owned()))
                .collect(),
        ),
    );
    entry.insert("context_window".into(), Value::Integer(context_window));
    entry.insert(
        "max_completion_tokens".into(),
        Value::Integer(max_output_tokens.into()),
    );
    entry.insert(
        "inference_idle_timeout_secs".into(),
        Value::Integer(inference_idle_timeout_secs),
    );
    entry.insert(
        "supports_backend_search".into(),
        Value::Boolean(supports_backend_search),
    );
    entry.insert("supported_in_api".into(), Value::Boolean(true));
    if !headers.is_empty() {
        entry.insert(
            "extra_headers".into(),
            Value::Table(
                headers
                    .into_iter()
                    .map(|(key, value)| (key, Value::String(value)))
                    .collect(),
            ),
        );
    }
    if !env_headers.is_empty() {
        entry.insert(
            "env_http_headers".into(),
            Value::Table(
                env_headers
                    .into_iter()
                    .map(|(key, value)| (key, Value::String(value)))
                    .collect(),
            ),
        );
    }

    let mut model_table = toml::map::Map::new();
    model_table.insert(RUNTIME_MODEL_ID.into(), Value::Table(entry));
    let mut models = toml::map::Map::new();
    models.insert("default".into(), Value::String(RUNTIME_MODEL_ID.into()));
    let mut root = toml::map::Map::new();
    root.insert("model".into(), Value::Table(model_table));
    root.insert("models".into(), Value::Table(models));
    Value::Table(root)
}

fn required(name: &str, value: Option<String>) -> Result<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("{name} is required with --backend"))
}

fn generate_secret() -> String {
    uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(24)
        .collect()
}

fn parse_key_value(value: &str) -> std::result::Result<(String, String), String> {
    let Some((key, value)) = value.split_once('=') else {
        return Err("expected KEY=VALUE".into());
    };
    if key.trim().is_empty() || value.is_empty() {
        return Err("header key and value must both be non-empty".into());
    }
    Ok((key.trim().to_owned(), value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_cli_starts_native_config_serve_mode() {
        let cli = Cli::try_parse_from(["grok-runtime"]).expect("bare serve CLI should parse");
        assert!(cli.backend.is_none());
        assert_eq!(cli.bind, "127.0.0.1:2419".parse().unwrap());
    }

    #[test]
    fn provider_arguments_require_backend() {
        let cli = Cli::try_parse_from(["grok-runtime", "--base-url", "https://gateway.example/v1"])
            .expect("provider arguments should reach runtime validation");
        let error = ServeRuntime::from_cli(cli)
            .err()
            .expect("provider arguments without --backend must fail");
        assert!(error.to_string().contains("--backend"));
    }

    #[test]
    fn anthropic_provider_patch_uses_original_model_config_surface() {
        let patch = provider_patch(
            Backend::Anthropic,
            "http://gateway/v1",
            "claude-sonnet-5",
            "ANTHROPIC_API_KEY",
            AuthScheme::Bearer,
            200_000,
            4_096,
            120,
            true,
            vec![("anthropic-version".into(), "2023-06-01".into())],
            Vec::new(),
        );
        let entry = &patch["model"][RUNTIME_MODEL_ID];
        assert_eq!(entry["api_backend"].as_str(), Some("messages"));
        assert_eq!(entry["auth_scheme"].as_str(), Some("bearer"));
        assert_eq!(entry["model"].as_str(), Some("claude-sonnet-5"));

        let config = AgentConfig::new_from_toml_cfg(&patch).expect("Grok config should parse");
        let model = config
            .config_models
            .get(RUNTIME_MODEL_ID)
            .expect("runtime model override should exist");
        assert_eq!(
            model.auth_scheme.map(|scheme| format!("{scheme:?}")),
            Some("Bearer".to_owned())
        );
    }
}
