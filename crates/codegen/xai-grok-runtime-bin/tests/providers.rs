use std::path::PathBuf;

use indexmap::IndexMap;
use xai_grok_runtime::{AgentRuntime, AuthMode, Backend, RuntimeConfig};
use xai_grok_test_support::{MockInferenceServer, MockModelEntry, ScriptedResponse, sse};

fn config(server: &MockInferenceServer, backend: Backend) -> RuntimeConfig {
    RuntimeConfig {
        backend,
        base_url: server.url(),
        model: "test-model".into(),
        api_key: "test-key".into(),
        auth_scheme: AuthMode::Bearer,
        headers: IndexMap::new(),
        env_headers: IndexMap::new(),
        cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        allow_write: false,
        allow_shell: false,
        max_turns: 2,
        max_output_tokens: 1024,
        tool_timeout_secs: 5,
        max_tool_output_bytes: 4096,
        system_prompt: None,
    }
}

#[tokio::test]
async fn responses_backend_completes_a_prompt() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("test-model").with_api_backend("responses"),
    ])
    .await
    .unwrap();
    server.set_response("hello from responses");

    let outcome = AgentRuntime::new(config(&server, Backend::Responses))
        .unwrap()
        .run_prompt("say hello")
        .await
        .unwrap();

    assert_eq!(outcome.text, "hello from responses");
    assert!(server.has_responses_request());
}

#[tokio::test]
async fn anthropic_backend_completes_a_prompt() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("test-model").with_api_backend("messages"),
    ])
    .await
    .unwrap();
    server.set_response("hello from messages");

    let outcome = AgentRuntime::new(config(&server, Backend::Anthropic))
        .unwrap()
        .run_prompt("say hello")
        .await
        .unwrap();

    assert_eq!(outcome.text, "hello from messages");
    assert_eq!(server.messages_request_count(), 1);
}

#[tokio::test]
async fn responses_backend_runs_a_local_tool_and_continues() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("test-model").with_api_backend("responses"),
    ])
    .await
    .unwrap();
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(sse::responses_api_reasoning_then_tool_call_events(
            "inspect the manifest",
            "call-read",
            "read_file",
            r#"{"path":"Cargo.toml","limit":2}"#,
            "test-model",
        )),
    );
    server.set_response("manifest inspected");

    let outcome = AgentRuntime::new(config(&server, Backend::Responses))
        .unwrap()
        .run_prompt("inspect the manifest")
        .await
        .unwrap();

    assert_eq!(outcome.text, "manifest inspected");
    assert_eq!(outcome.turns, 2);
    assert_eq!(outcome.tool_calls, 1);
    let bodies = server.request_bodies();
    assert_eq!(bodies.len(), 2);
    assert!(
        bodies[1].to_string().contains("function_call_output"),
        "second request should return the tool result: {}",
        bodies[1]
    );
}
