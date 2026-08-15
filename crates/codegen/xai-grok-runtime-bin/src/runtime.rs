use anyhow::{Context, Result, bail};
use xai_grok_sampler::SamplingClient;
use xai_grok_sampling_types::{ConversationItem, ConversationRequest};

use crate::config::RuntimeConfig;
use crate::tools::ToolRuntime;

pub struct AgentRuntime {
    client: SamplingClient,
    config: RuntimeConfig,
    tools: ToolRuntime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOutcome {
    pub text: String,
    pub turns: u32,
    pub tool_calls: u32,
}

impl AgentRuntime {
    pub fn new(config: RuntimeConfig) -> Result<Self> {
        let client =
            SamplingClient::new(config.sampler_config()).context("create inference client")?;
        let tools = ToolRuntime::new(
            config.cwd.clone(),
            config.allow_write,
            config.allow_shell,
            config.tool_timeout_secs,
            config.max_tool_output_bytes,
        );
        Ok(Self {
            client,
            config,
            tools,
        })
    }

    pub async fn run_prompt(&self, prompt: impl Into<String>) -> Result<RunOutcome> {
        let mut history = vec![
            ConversationItem::system(self.system_prompt()),
            ConversationItem::user(prompt.into()),
        ];
        let mut tool_call_count = 0u32;

        for turn in 1..=self.config.max_turns {
            let mut request =
                ConversationRequest::from_items(history.clone()).with_tools(self.tools.specs());
            request.max_output_tokens = Some(self.config.max_output_tokens);

            let response = self
                .client
                .conversation_collect(request)
                .await
                .with_context(|| format!("inference failed on turn {turn}"))?;
            let assistant = response
                .assistant()
                .context("provider response did not contain an assistant item")?;
            let text = assistant.content.as_ref().to_owned();
            let tool_calls = assistant.tool_calls.clone();
            history.extend(response.items);

            if tool_calls.is_empty() {
                return Ok(RunOutcome {
                    text,
                    turns: turn,
                    tool_calls: tool_call_count,
                });
            }

            for call in tool_calls {
                tool_call_count = tool_call_count.saturating_add(1);
                eprintln!("[tool] {}", call.name);
                let result = self.tools.execute(&call).await;
                history.push(ConversationItem::tool_result(call.id.as_ref(), result));
            }
        }

        bail!(
            "agent exceeded the configured maximum of {} turns",
            self.config.max_turns
        )
    }

    fn system_prompt(&self) -> String {
        let mut prompt = format!(
            "You are a compact coding agent operating in the workspace {}. \
Use the available tools to inspect the real files before answering. Keep edits focused, \
preserve unrelated work, and report what you verified. Never claim a tool action succeeded \
unless its result says so.",
            self.config.cwd.display()
        );
        if !self.config.allow_write {
            prompt.push_str(" File writes are disabled for this run.");
        }
        if !self.config.allow_shell {
            prompt.push_str(" Shell execution is disabled for this run.");
        }
        if let Some(extra) = self.config.system_prompt.as_deref() {
            prompt.push_str("\n\nAdditional instructions:\n");
            prompt.push_str(extra);
        }
        prompt
    }
}
