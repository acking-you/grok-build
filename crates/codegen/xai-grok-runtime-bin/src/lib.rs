//! Serve-only composition root for the original Grok Build runtime.

mod serve;

pub use serve::{AuthScheme, Backend, Cli, ServeRuntime};
