use std::path::PathBuf;

use indexmap::IndexMap;
use serde_json::json;
use xai_grok_runtime::{AgentRuntime, AuthMode, Backend, RuntimeConfig};
use xai_grok_test_support::{MockInferenceServer, MockModelEntry, ScriptedResponse, SseEvent, sse};

const WEB_SEARCH_TOOL_NAME: &str = "web_search_runtime";

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
        inference_timeout_secs: 5,
        max_tool_output_bytes: 4096,
        system_prompt: None,
    }
}

#[tokio::test]
async fn model_request_has_a_total_timeout() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("test-model").with_api_backend("messages"),
    ])
    .await
    .unwrap();
    server.hold_agent_completions();
    let mut runtime_config = config(&server, Backend::Anthropic);
    runtime_config.inference_timeout_secs = 1;

    let error = AgentRuntime::new(runtime_config)
        .unwrap()
        .run_prompt("never finish")
        .await
        .unwrap_err();
    server.release_agent_completions();

    assert!(
        error.to_string().contains("inference failed on turn 1"),
        "{error:#}"
    );
    assert!(
        format!("{error:#}").contains("agent-turn-1 timed out after 1 seconds"),
        "{error:#}"
    );
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

#[tokio::test]
async fn responses_backend_runs_hosted_web_search_and_returns_to_agent() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("test-model").with_api_backend("responses"),
    ])
    .await
    .unwrap();
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(sse::responses_api_reasoning_then_tool_call_events(
            "fresh information needed",
            "call-search",
            WEB_SEARCH_TOOL_NAME,
            r#"{"query":"current Singapore weather"}"#,
            "test-model",
        )),
    );
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(sse::responses_api_script_exact(
            "Singapore is sunny. https://weather.example",
            "test-model",
        )),
    );
    server.set_response("weather reported");

    let outcome = AgentRuntime::new(config(&server, Backend::Responses))
        .unwrap()
        .run_prompt("what is the weather?")
        .await
        .unwrap();

    assert_eq!(outcome.text, "weather reported");
    assert_eq!(outcome.tool_calls, 1);
    let bodies = server.request_bodies();
    assert_eq!(bodies.len(), 3);
    assert!(
        bodies[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == WEB_SEARCH_TOOL_NAME)
    );
    assert_eq!(bodies[1]["tools"], json!([{"type":"web_search"}]));
    assert!(bodies[2].to_string().contains("function_call_output"));
    assert!(bodies[2].to_string().contains("weather.example"));
}

#[tokio::test]
async fn anthropic_backend_runs_hosted_web_search_and_returns_to_agent() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("test-model").with_api_backend("messages"),
    ])
    .await
    .unwrap();
    server.enqueue_response(
        "/v1/messages",
        ScriptedResponse::sse(anthropic_tool_call_events(
            "call-search",
            WEB_SEARCH_TOOL_NAME,
            r#"{"query":"current Singapore weather"}"#,
        )),
    );
    server.enqueue_response(
        "/v1/messages",
        ScriptedResponse::sse(anthropic_web_search_events()),
    );
    server.set_response("weather reported");

    let outcome = AgentRuntime::new(config(&server, Backend::Anthropic))
        .unwrap()
        .run_prompt("what is the weather?")
        .await
        .unwrap();

    assert_eq!(outcome.text, "weather reported");
    assert_eq!(outcome.tool_calls, 1);
    let bodies = server.request_bodies();
    assert_eq!(bodies.len(), 3);
    assert!(
        bodies[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == WEB_SEARCH_TOOL_NAME)
    );
    assert_eq!(
        bodies[1]["tools"],
        json!([{
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 8
        }])
    );
    assert!(
        bodies[1]
            .to_string()
            .contains("Perform a web search for the query: current Singapore weather")
    );
    assert!(bodies[2].to_string().contains("tool_result"));
    assert!(bodies[2].to_string().contains("weather.example"));
}

fn anthropic_tool_call_events(id: &str, name: &str, arguments: &str) -> Vec<SseEvent> {
    vec![
        message_start_event(),
        event(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type":"tool_use","id":id,"name":name,"input":{}}
        })),
        event(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type":"input_json_delta","partial_json":arguments}
        })),
        event(json!({"type":"content_block_stop","index":0})),
        message_delta_event("tool_use"),
        event(json!({"type":"message_stop"})),
    ]
}

fn anthropic_web_search_events() -> Vec<SseEvent> {
    vec![
        message_start_event(),
        event(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type":"server_tool_use", "id":"srvtoolu_search",
                "name":"web_search", "input":{}
            }
        })),
        event(json!({
            "type":"content_block_delta", "index":0,
            "delta":{"type":"input_json_delta","partial_json":"{\"query\":\"current Singapore weather\"}"}
        })),
        event(json!({"type":"content_block_stop","index":0})),
        event(json!({
            "type":"content_block_start", "index":1,
            "content_block": {
                "type":"web_search_tool_result", "tool_use_id":"srvtoolu_search",
                "content":[{
                    "type":"web_search_result", "title":"Weather", "url":"https://weather.example",
                    "encrypted_content":"sunny"
                }]
            }
        })),
        event(json!({"type":"content_block_stop","index":1})),
        event(json!({
            "type":"content_block_start", "index":2,
            "content_block":{"type":"text","text":""}
        })),
        event(json!({
            "type":"content_block_delta", "index":2,
            "delta":{"type":"text_delta","text":"Singapore is sunny. https://weather.example"}
        })),
        event(json!({"type":"content_block_stop","index":2})),
        message_delta_event("end_turn"),
        event(json!({"type":"message_stop"})),
    ]
}

fn message_start_event() -> SseEvent {
    event(json!({
        "type":"message_start",
        "message": {
            "id":"msg_test", "type":"message", "role":"assistant", "content":[],
            "model":"test-model", "stop_reason":null,
            "usage": {
                "input_tokens":10, "output_tokens":0,
                "cache_creation_input_tokens":0, "cache_read_input_tokens":0
            }
        }
    }))
}

fn message_delta_event(stop_reason: &str) -> SseEvent {
    event(json!({
        "type":"message_delta", "delta":{"stop_reason":stop_reason},
        "usage":{"output_tokens":5,"input_tokens":10}
    }))
}

fn event(value: serde_json::Value) -> SseEvent {
    SseEvent::data(value.to_string())
}
