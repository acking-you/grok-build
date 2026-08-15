use anyhow::{Context, Result, bail};
use std::time::{Duration, Instant};
use xai_grok_sampler::SamplingClient;
use xai_grok_sampling_types::{
    ConversationItem, ConversationRequest, ConversationResponse, HostedTool, ToolCall,
};

use crate::config::RuntimeConfig;
use crate::tools::{ToolRuntime, WEB_SEARCH_TOOL_NAME};

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
                .collect_inference(&format!("agent-turn-{turn}"), request)
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
                eprintln!(
                    "[tool:start] turn={turn} sequence={tool_call_count} id={} name={} {}",
                    call.id,
                    call.name,
                    self.tools.describe_call(&call)
                );
                let started = Instant::now();
                let result = if call.name == WEB_SEARCH_TOOL_NAME {
                    let search = self.run_web_search(&call).await;
                    self.tools.format_result(search)
                } else {
                    self.tools.execute(&call).await
                };
                let status = if result.starts_with("Error:") {
                    "error"
                } else {
                    "ok"
                };
                eprintln!(
                    "[tool:done] turn={turn} sequence={tool_call_count} id={} name={} status={status} elapsed_ms={} output_bytes={} preview={}",
                    call.id,
                    call.name,
                    started.elapsed().as_millis(),
                    result.len(),
                    preview_for_log(&result, 240)
                );
                history.push(ConversationItem::tool_result(call.id.as_ref(), result));
            }
        }

        bail!(
            "agent exceeded the configured maximum of {} turns",
            self.config.max_turns
        )
    }

    async fn run_web_search(&self, call: &ToolCall) -> Result<String> {
        let query = self.tools.web_search_query(call)?;
        let prompt = match self.config.backend {
            crate::config::Backend::Anthropic => {
                format!("Perform a web search for the query: {query}")
            }
            crate::config::Backend::Responses => format!(
                "Search the web for the following query and return a concise answer with source URLs: {query}"
            ),
        };
        let mut request = ConversationRequest::from_items(vec![ConversationItem::user(prompt)]);
        request.hosted_tools = vec![HostedTool::WebSearch { options: None }];
        request.max_output_tokens = Some(self.config.max_output_tokens);

        let response = self
            .collect_inference("hosted-web-search", request)
            .await
            .context("hosted web search failed")?;
        let answer = response
            .assistant()
            .context("web search response did not contain an assistant item")?
            .content
            .trim()
            .to_owned();
        if answer.is_empty() {
            bail!("web search returned no text result");
        }
        Ok(answer)
    }

    async fn collect_inference(
        &self,
        phase: &str,
        request: ConversationRequest,
    ) -> Result<ConversationResponse> {
        let backend = match self.config.backend {
            crate::config::Backend::Responses => "responses",
            crate::config::Backend::Anthropic => "anthropic",
        };
        eprintln!(
            "[model:start] phase={phase} backend={backend} model={} input_items={} tools={} timeout_secs={}",
            preview_for_log(&self.config.model, 200),
            request.items.len(),
            request.tools.len() + request.hosted_tools.len(),
            self.config.inference_timeout_secs
        );
        let started = Instant::now();
        let timeout_after = Duration::from_secs(self.config.inference_timeout_secs);
        match tokio::time::timeout(timeout_after, self.client.conversation_collect(request)).await {
            Ok(Ok(response)) => {
                let (text_bytes, tool_calls) = response
                    .assistant()
                    .map(|assistant| (assistant.content.len(), assistant.tool_calls.len()))
                    .unwrap_or_default();
                eprintln!(
                    "[model:done] phase={phase} status=ok elapsed_ms={} output_text_bytes={text_bytes} tool_calls={tool_calls}",
                    started.elapsed().as_millis()
                );
                Ok(response)
            }
            Ok(Err(error)) => {
                eprintln!(
                    "[model:error] phase={phase} status=error elapsed_ms={} error={}",
                    started.elapsed().as_millis(),
                    preview_for_log(&error.to_string(), 500)
                );
                Err(error.into())
            }
            Err(_) => {
                eprintln!(
                    "[model:error] phase={phase} status=timeout elapsed_ms={} timeout_secs={}",
                    started.elapsed().as_millis(),
                    self.config.inference_timeout_secs
                );
                bail!(
                    "{phase} timed out after {} seconds",
                    self.config.inference_timeout_secs
                )
            }
        }
    }

    fn system_prompt(&self) -> String {
        let mut prompt = format!(
            "You are a compact coding agent operating in the workspace {}. \
Use the available tools to inspect the real files before answering. Keep edits focused, \
preserve unrelated work, and report what you verified. Never claim a tool action succeeded \
unless its result says so. Use web_search_runtime whenever the answer depends on current or \
external information.",
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

fn preview_for_log(value: &str, max_chars: usize) -> String {
    serde_json::to_string(&bounded_preview(value, max_chars))
        .unwrap_or_else(|_| "\"<unprintable>\"".into())
}

fn bounded_preview(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}
