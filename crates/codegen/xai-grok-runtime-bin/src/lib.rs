//! A small, headless coding-agent runtime.

pub mod config;
mod runtime;
mod tools;

pub use config::{AuthMode, Backend, Cli, ResolvedInput, RuntimeConfig};
pub use runtime::{AgentRuntime, RunOutcome};
