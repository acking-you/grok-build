//! Tool definition types for the model API.
//!
//! The canonical types live in the lightweight `xai-tool-types` crate so
//! protocol-only consumers do not pull in every Grok tool implementation.

pub use xai_tool_types::{FunctionTool, ToolDefinition, ToolType};
