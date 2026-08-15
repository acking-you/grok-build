use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use indexmap::IndexMap;
use serde::Deserialize;
use xai_grok_sampler::{ApiBackend, AuthScheme, SamplerConfig};

const DEFAULT_MAX_TURNS: u32 = 16;
const DEFAULT_RESPONSES_MAX_OUTPUT_TOKENS: u32 = 8_192;
const DEFAULT_ANTHROPIC_MAX_OUTPUT_TOKENS: u32 = 4_096;
const DEFAULT_INFERENCE_TIMEOUT_SECS: u64 = 120;
const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Responses,
    #[value(alias = "messages")]
    #[serde(alias = "messages")]
    Anthropic,
}

impl Backend {
    pub fn api_backend(self) -> ApiBackend {
        match self {
            Self::Responses => ApiBackend::Responses,
            Self::Anthropic => ApiBackend::Messages,
        }
    }

    fn default_key_env(self) -> &'static str {
        match self {
            Self::Responses => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
        }
    }

    fn default_auth(self) -> AuthMode {
        match self {
            Self::Responses => AuthMode::Bearer,
            Self::Anthropic => AuthMode::XApiKey,
        }
    }

    fn default_max_output_tokens(self) -> u32 {
        match self {
            Self::Responses => DEFAULT_RESPONSES_MAX_OUTPUT_TOKENS,
            Self::Anthropic => DEFAULT_ANTHROPIC_MAX_OUTPUT_TOKENS,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Bearer,
    #[value(name = "x-api-key", alias = "x_api_key")]
    XApiKey,
}

impl AuthMode {
    fn sampler_scheme(self) -> AuthScheme {
        match self {
            Self::Bearer => AuthScheme::Bearer,
            Self::XApiKey => AuthScheme::XApiKey,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "grok-runtime",
    about = "Minimal Grok coding runtime for third-party model APIs"
)]
pub struct Cli {
    /// TOML configuration file. Command-line values override file values.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Inference protocol: OpenAI Responses or Anthropic Messages.
    #[arg(long, value_enum)]
    pub backend: Option<Backend>,

    /// Provider base URL, normally ending in /v1.
    #[arg(long, env = "GROK_RUNTIME_BASE_URL")]
    pub base_url: Option<String>,

    /// Provider model identifier.
    #[arg(long, env = "GROK_RUNTIME_MODEL")]
    pub model: Option<String>,

    /// Environment variable containing the API key.
    #[arg(long, env = "GROK_RUNTIME_API_KEY_ENV")]
    pub api_key_env: Option<String>,

    /// Authentication header style. Defaults by backend.
    #[arg(long, value_enum)]
    pub auth_scheme: Option<AuthMode>,

    /// Literal HTTP header in KEY=VALUE form. Repeatable.
    #[arg(long = "header", value_parser = parse_key_value)]
    pub headers: Vec<(String, String)>,

    /// Secret HTTP header in KEY=ENV_VAR form. Repeatable.
    #[arg(long = "header-env", value_parser = parse_key_value)]
    pub env_headers: Vec<(String, String)>,

    /// Workspace root exposed to file tools.
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Allow write_file and edit_file inside the workspace root.
    #[arg(long)]
    pub allow_write: bool,

    /// Enable shell execution (already the default unless disabled in TOML).
    #[arg(long, conflicts_with = "no_shell")]
    pub allow_shell: bool,

    /// Disable shell execution. Shell is enabled by default for this runtime.
    #[arg(long, conflicts_with = "allow_shell")]
    pub no_shell: bool,

    /// Maximum model/tool turns before stopping.
    #[arg(long)]
    pub max_turns: Option<u32>,

    /// Maximum completion tokens requested from the provider.
    #[arg(long)]
    pub max_output_tokens: Option<u32>,

    /// Timeout for search, listing, and shell tools.
    #[arg(long)]
    pub tool_timeout_secs: Option<u64>,

    /// Total timeout for each model or hosted web-search request.
    #[arg(long)]
    pub inference_timeout_secs: Option<u64>,

    /// Maximum bytes returned by one tool call.
    #[arg(long)]
    pub max_tool_output_bytes: Option<usize>,

    /// Additional system instructions appended after the built-in coding prompt.
    #[arg(long)]
    pub system_prompt: Option<String>,

    /// User request. If omitted, it is read from piped stdin.
    #[arg(short = 'p', long)]
    pub prompt: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    backend: Option<Backend>,
    base_url: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    auth_scheme: Option<AuthMode>,
    cwd: Option<PathBuf>,
    allow_write: Option<bool>,
    allow_shell: Option<bool>,
    max_turns: Option<u32>,
    max_output_tokens: Option<u32>,
    tool_timeout_secs: Option<u64>,
    inference_timeout_secs: Option<u64>,
    max_tool_output_bytes: Option<usize>,
    system_prompt: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    env_headers: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct RuntimeConfig {
    pub backend: Backend,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub auth_scheme: AuthMode,
    pub headers: IndexMap<String, String>,
    pub env_headers: IndexMap<String, String>,
    pub cwd: PathBuf,
    pub allow_write: bool,
    pub allow_shell: bool,
    pub max_turns: u32,
    pub max_output_tokens: u32,
    pub tool_timeout_secs: u64,
    pub inference_timeout_secs: u64,
    pub max_tool_output_bytes: usize,
    pub system_prompt: Option<String>,
}

pub struct ResolvedInput {
    pub config: RuntimeConfig,
    pub prompt: String,
}

impl ResolvedInput {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        let file = load_file(cli.config.as_ref())?;
        let backend = cli.backend.or(file.backend).unwrap_or(Backend::Responses);
        let base_url = required("base_url", cli.base_url.or(file.base_url))?;
        let model = required("model", cli.model.or(file.model))?;
        let key_env = cli
            .api_key_env
            .or(file.api_key_env)
            .unwrap_or_else(|| backend.default_key_env().to_owned());
        let api_key = std::env::var("GROK_RUNTIME_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var(&key_env)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .with_context(|| {
                format!(
                    "API key is missing; set GROK_RUNTIME_API_KEY or the configured variable {key_env}"
                )
            })?;

        let mut headers: IndexMap<_, _> = file.headers.into_iter().collect();
        headers.extend(cli.headers);
        if backend == Backend::Anthropic
            && !headers
                .keys()
                .any(|key| key.eq_ignore_ascii_case("anthropic-version"))
        {
            headers.insert("anthropic-version".into(), "2023-06-01".into());
        }
        let mut env_headers: IndexMap<_, _> = file.env_headers.into_iter().collect();
        env_headers.extend(cli.env_headers);

        let cwd = cli
            .cwd
            .or(file.cwd)
            .unwrap_or(std::env::current_dir().context("resolve current directory")?);
        let cwd = dunce::canonicalize(&cwd)
            .with_context(|| format!("workspace root does not exist: {}", cwd.display()))?;
        if !cwd.is_dir() {
            bail!("workspace root is not a directory: {}", cwd.display());
        }

        let max_turns = cli
            .max_turns
            .or(file.max_turns)
            .unwrap_or(DEFAULT_MAX_TURNS);
        if max_turns == 0 {
            bail!("max_turns must be greater than zero");
        }
        let max_output_tokens = cli
            .max_output_tokens
            .or(file.max_output_tokens)
            .unwrap_or_else(|| backend.default_max_output_tokens());
        if max_output_tokens == 0 {
            bail!("max_output_tokens must be greater than zero");
        }
        let tool_timeout_secs = cli
            .tool_timeout_secs
            .or(file.tool_timeout_secs)
            .unwrap_or(DEFAULT_TOOL_TIMEOUT_SECS);
        if tool_timeout_secs == 0 {
            bail!("tool_timeout_secs must be greater than zero");
        }
        let inference_timeout_secs = cli
            .inference_timeout_secs
            .or(file.inference_timeout_secs)
            .unwrap_or(DEFAULT_INFERENCE_TIMEOUT_SECS);
        if inference_timeout_secs == 0 {
            bail!("inference_timeout_secs must be greater than zero");
        }
        let max_tool_output_bytes = cli
            .max_tool_output_bytes
            .or(file.max_tool_output_bytes)
            .unwrap_or(DEFAULT_MAX_TOOL_OUTPUT_BYTES);
        if max_tool_output_bytes == 0 {
            bail!("max_tool_output_bytes must be greater than zero");
        }

        let prompt = resolve_prompt(cli.prompt)?;
        Ok(Self {
            config: RuntimeConfig {
                backend,
                base_url,
                model,
                api_key,
                auth_scheme: cli
                    .auth_scheme
                    .or(file.auth_scheme)
                    .unwrap_or_else(|| backend.default_auth()),
                headers,
                env_headers,
                cwd,
                allow_write: cli.allow_write || file.allow_write.unwrap_or(false),
                allow_shell: if cli.no_shell {
                    false
                } else if cli.allow_shell {
                    true
                } else {
                    file.allow_shell.unwrap_or(true)
                },
                max_turns,
                max_output_tokens,
                tool_timeout_secs,
                inference_timeout_secs,
                max_tool_output_bytes,
                system_prompt: cli.system_prompt.or(file.system_prompt),
            },
            prompt,
        })
    }
}

impl RuntimeConfig {
    pub(crate) fn sampler_config(&self) -> SamplerConfig {
        SamplerConfig {
            api_key: Some(self.api_key.clone()),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            max_completion_tokens: Some(self.max_output_tokens),
            api_backend: self.backend.api_backend(),
            auth_scheme: self.auth_scheme.sampler_scheme(),
            extra_headers: self.headers.clone(),
            env_http_headers: self.env_headers.clone(),
            context_window: 0,
            ..SamplerConfig::default()
        }
    }
}

fn load_file(path: Option<&PathBuf>) -> Result<FileConfig> {
    let Some(path) = path else {
        return Ok(FileConfig::default());
    };
    let content = fs::read_to_string(path)
        .with_context(|| format!("read configuration file {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parse configuration file {}", path.display()))
}

fn required(name: &str, value: Option<String>) -> Result<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("{name} is required (set it in the config file or command line)"))
}

fn resolve_prompt(prompt: Option<String>) -> Result<String> {
    if let Some(prompt) = prompt.filter(|prompt| !prompt.trim().is_empty()) {
        return Ok(prompt);
    }
    if io::stdin().is_terminal() {
        bail!("a prompt is required; pass --prompt or pipe text on stdin");
    }
    let mut prompt = String::new();
    io::stdin()
        .read_to_string(&mut prompt)
        .context("read prompt from stdin")?;
    if prompt.trim().is_empty() {
        bail!("the prompt read from stdin is empty");
    }
    Ok(prompt)
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
    fn backend_output_defaults_avoid_anthropic_large_cap_stalls() {
        assert_eq!(
            Backend::Responses.default_max_output_tokens(),
            DEFAULT_RESPONSES_MAX_OUTPUT_TOKENS
        );
        assert_eq!(
            Backend::Anthropic.default_max_output_tokens(),
            DEFAULT_ANTHROPIC_MAX_OUTPUT_TOKENS
        );
        assert!(
            Backend::Anthropic.default_max_output_tokens()
                < Backend::Responses.default_max_output_tokens()
        );
    }
}
