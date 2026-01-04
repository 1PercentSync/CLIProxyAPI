//! OpenAI protocol thinking injection.
//!
//! This module handles injecting thinking configuration into OpenAI API requests.
//! It supports both `/v1/chat/completions` and `/v1/responses` endpoints.

use crate::thinking::ThinkingConfig;

/// Inject OpenAI thinking configuration into request body.
///
/// # Arguments
/// * `body` - Request body JSON
/// * `base_model` - Base model name (suffix stripped)
/// * `thinking_config` - Processed thinking config (Effort or Disabled)
/// * `is_responses_endpoint` - Whether this is a Responses endpoint (determined by caller)
///
/// # Panics
/// Panics if `thinking_config` is `ThinkingConfig::Budget`.
/// The injector should always convert to Effort or Disabled for OpenAI protocol.
pub fn inject_openai(
    mut body: serde_json::Value,
    base_model: &str,
    thinking_config: ThinkingConfig,
    is_responses_endpoint: bool,
) -> serde_json::Value {
    // Update model name (strip suffix)
    body["model"] = serde_json::Value::String(base_model.to_string());

    let effort = match thinking_config {
        ThinkingConfig::Disabled => "none".to_string(),
        ThinkingConfig::Effort(e) => e,
        ThinkingConfig::Budget(_) => {
            // OpenAI protocol should not receive Budget type, injector should have converted
            unreachable!("OpenAI protocol should receive Effort or Disabled, not Budget")
        }
    };

    // Inject based on endpoint type
    if is_responses_endpoint {
        // /v1/responses uses nested structure
        if body.get("reasoning").is_none() {
            body["reasoning"] = serde_json::json!({});
        }
        body["reasoning"]["effort"] = serde_json::Value::String(effort);
    } else {
        // /v1/chat/completions uses top-level field
        body["reasoning_effort"] = serde_json::Value::String(effort);
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_inject_chat_completions() {
        let body = json!({
            "model": "gpt-5.1(high)",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = inject_openai(
            body,
            "gpt-5.1",
            ThinkingConfig::Effort("high".to_string()),
            false,
        );

        assert_eq!(result["model"], "gpt-5.1");
        assert_eq!(result["reasoning_effort"], "high");
        assert!(result.get("reasoning").is_none());
    }

    #[test]
    fn test_inject_responses() {
        let body = json!({
            "model": "gpt-5.1(medium)",
            "input": "Hello"
        });

        let result = inject_openai(
            body,
            "gpt-5.1",
            ThinkingConfig::Effort("medium".to_string()),
            true,
        );

        assert_eq!(result["model"], "gpt-5.1");
        assert_eq!(result["reasoning"]["effort"], "medium");
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_override_existing_effort() {
        let body = json!({
            "model": "gpt-5.1(high)",
            "messages": [],
            "reasoning_effort": "low"
        });

        let result = inject_openai(
            body,
            "gpt-5.1",
            ThinkingConfig::Effort("high".to_string()),
            false,
        );

        assert_eq!(result["reasoning_effort"], "high");
    }

    #[test]
    fn test_override_existing_responses_effort() {
        let body = json!({
            "model": "gpt-5.1(high)",
            "input": "test",
            "reasoning": {"effort": "low"}
        });

        let result = inject_openai(
            body,
            "gpt-5.1",
            ThinkingConfig::Effort("high".to_string()),
            true,
        );

        assert_eq!(result["reasoning"]["effort"], "high");
    }

    #[test]
    fn test_inject_disabled() {
        let body = json!({
            "model": "gpt-5.1(none)",
            "messages": []
        });

        let result = inject_openai(
            body,
            "gpt-5.1",
            ThinkingConfig::Disabled,
            false,
        );

        assert_eq!(result["model"], "gpt-5.1");
        assert_eq!(result["reasoning_effort"], "none");
    }
}
