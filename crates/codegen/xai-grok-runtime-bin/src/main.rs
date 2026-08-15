use anyhow::Result;
use clap::Parser;
use xai_grok_runtime::{AgentRuntime, Cli, ResolvedInput};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("grok-runtime: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let input = ResolvedInput::from_cli(Cli::parse())?;
    let runtime = AgentRuntime::new(input.config)?;
    let outcome = runtime.run_prompt(input.prompt).await?;
    println!("{}", outcome.text);
    Ok(())
}
