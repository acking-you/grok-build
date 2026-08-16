use anyhow::Result;
use clap::Parser;
use xai_grok_runtime::{Cli, ServeRuntime};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("grok-runtime: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    init_tracing();
    ServeRuntime::from_cli(Cli::parse())?.run().await
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}
