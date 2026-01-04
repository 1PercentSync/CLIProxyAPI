//! Anthropic protocol thinking injection.
//!
//! This module handles injecting thinking configuration into Anthropic API requests.

use crate::models::registry::ModelInfo;
use crate::thinking::ThinkingConfig;

/// Inject Anthropic thinking configuration into request body.
///
/// # Arguments
/// * `body` - Request body JSON
/// * `base_model` - Base model name (suffix stripped)
/// * `thinking_config` - Processed thinking config (Budget or Disabled)
/// * `model_info` - Model information for max_tokens adjustment
///
/// # Panics
/// Panics if `thinking_config` is `ThinkingConfig::Effort`.
/// The injector should always convert to Budget or Disabled for Anthropic protocol.
pub fn inject_anthropic(
    mut body: serde_json::Value,
    base_model: &str,
    thinking_config: ThinkingConfig,
    model_info: &ModelInfo,
) -> serde_json::Value {
    // Update model name (strip suffix)
    body["model"] = serde_json::Value::String(base_model.to_string());

    match thinking_config {
        ThinkingConfig::Disabled => {
            // Explicitly disable thinking
            body["thinking"] = serde_json::json!({
                "type": "disabled"
            });
        }
        ThinkingConfig::Budget(budget) if budget > 0 => {
            // Set thinking config
            if body.get("thinking").is_none() {
                body["thinking"] = serde_json::json!({});
            }
            body["thinking"]["type"] = serde_json::Value::String("enabled".to_string());
            body["thinking"]["budget_tokens"] = serde_json::Value::Number(budget.into());

            // Adjust max_tokens if needed
            // Anthropic API requires max_tokens > thinking.budget_tokens
            let current_max = body
                .get("max_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let required_max = model_info.max_completion_tokens as i64;

            if current_max < required_max {
                body["max_tokens"] = serde_json::Value::Number(required_max.into());
            }
        }
        ThinkingConfig::Budget(_) => {
            // budget <= 0 (like -1 for dynamic): just return body with updated model
            // This shouldn't happen as injector should have converted -1 to auto_budget
        }
        ThinkingConfig::Effort(_) => {
            unreachable!("Anthropic protocol should receive Budget or Disabled, not Effort")
        }
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::registry::ThinkingSupport;
    use serde_json::json;

    fn make_model_info() -> ModelInfo {
        ModelInfo {
            id: "claude-sonnet-4-5-20250929",
            max_completion_tokens: 64000,
            thinking: Some(ThinkingSupport {
                min: 1024,
                max: 100000,
                zero_allowed: false,
                dynamic_allowed: false, // Claude API doesn't support budget_tokens=-1
                auto_budget: Some(16384), // Default for (auto) since dynamic not supported
                levels: None,
            }),
        }
    }

    #[test]
    fn test_inject_positive_budget() {
        let body = json!({
            "model": "claude-sonnet-4-5-20250929(16384)",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = inject_anthropic(
            body,
            "claude-sonnet-4-5-20250929",
            ThinkingConfig::Budget(16384),
            &make_model_info(),
        );

        assert_eq!(result["model"], "claude-sonnet-4-5-20250929");
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 16384);
        assert_eq!(result["max_tokens"], 64000);
    }

    #[test]
    fn test_inject_disabled() {
        let body = json!({
            "model": "claude-sonnet-4-5-20250929(none)",
            "messages": []
        });

        let result = inject_anthropic(
            body,
            "claude-sonnet-4-5-20250929",
            ThinkingConfig::Disabled,
            &make_model_info(),
        );

        assert_eq!(result["model"], "claude-sonnet-4-5-20250929");
        assert_eq!(result["thinking"]["type"], "disabled");
        assert!(result["thinking"].get("budget_tokens").is_none());
    }

    #[test]
    fn test_inject_negative_budget() {
        let body = json!({
            "model": "claude-sonnet-4-5-20250929(-1)",
            "messages": []
        });

        let result = inject_anthropic(
            body,
            "claude-sonnet-4-5-20250929",
            ThinkingConfig::Budget(-1),
            &make_model_info(),
        );

        assert_eq!(result["model"], "claude-sonnet-4-5-20250929");
        assert!(result.get("thinking").is_none());
    }

    #[test]
    fn test_preserve_higher_max_tokens() {
        let body = json!({
            "model": "claude-sonnet-4-5-20250929(8192)",
            "messages": [],
            "max_tokens": 100000
        });

        let result = inject_anthropic(
            body,
            "claude-sonnet-4-5-20250929",
            ThinkingConfig::Budget(8192),
            &make_model_info(),
        );

        // Should preserve user's higher max_tokens
        assert_eq!(result["max_tokens"], 100000);
    }

    #[test]
    fn test_override_existing_thinking() {
        let body = json!({
            "model": "claude-sonnet-4-5-20250929(high)",
            "messages": [],
            "thinking": {"type": "disabled", "budget_tokens": 0}
        });

        let result = inject_anthropic(
            body,
            "claude-sonnet-4-5-20250929",
            ThinkingConfig::Budget(24576),
            &make_model_info(),
        );

        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 24576);
    }
}
