//! Unified thinking configuration injection.
//!
//! This module coordinates the entire thinking injection workflow:
//! parsing, validation, mapping, clamping, and protocol-specific injection.

use crate::models::registry::{ModelInfo, get_model_info};
use crate::protocol::{Protocol, inject_anthropic, inject_gemini, inject_openai};
use crate::thinking::ThinkingConfig;
use crate::thinking::models::{
    budget_to_effort, clamp_budget, clamp_effort_to_levels, level_to_budget,
};
use crate::thinking::parser::{ThinkingValue, parse_model_suffix};

/// Check if path is an OpenAI Responses endpoint.
fn is_responses_endpoint(path: &str) -> bool {
    path.contains("/responses")
}

/// Injection result.
#[derive(Debug)]
pub enum InjectionResult {
    /// Successfully injected, returns modified request body.
    Injected(serde_json::Value),
    /// No injection needed (model doesn't support or no suffix).
    PassThrough(serde_json::Value),
    /// Error (unknown model with suffix).
    Error(InjectionError),
}

/// Injection error.
#[derive(Debug, Clone)]
pub struct InjectionError {
    pub status: u16,
    pub message: String,
}

/// Unified injection entry point.
///
/// Coordinates the entire thinking injection workflow:
/// 1. Parse model suffix
/// 2. Validate model exists in registry
/// 3. Check thinking support
/// 4. Map and clamp values
/// 5. Protocol-specific injection
///
/// # Arguments
/// * `body` - Request body JSON
/// * `model_with_suffix` - Model name (may include suffix like `model(high)`)
/// * `protocol` - Detected protocol type
/// * `request_path` - Request path (for OpenAI endpoint type detection)
pub fn inject_thinking_config(
    body: serde_json::Value,
    model_with_suffix: &str,
    protocol: Protocol,
    request_path: &str,
) -> InjectionResult {
    // 1. Parse suffix
    let parsed = parse_model_suffix(model_with_suffix);
    let base_model = parsed.base_name;

    // 2. No suffix or empty suffix: strip parentheses and passthrough
    let thinking_value = match parsed.thinking {
        ThinkingValue::None => {
            // Empty parentheses or no suffix: update model name and passthrough
            let mut body = body;
            body["model"] = serde_json::Value::String(base_model);
            return InjectionResult::PassThrough(body);
        }
        v => v,
    };

    // 3. Check if model is known
    let model_info = match get_model_info(&base_model) {
        Some(info) => info,
        None => {
            return InjectionResult::Error(InjectionError {
                status: 400,
                message: format!("unknown model with thinking suffix: {}", model_with_suffix),
            });
        }
    };

    // 4. Check if model supports thinking
    if model_info.thinking.is_none() {
        // Known model but doesn't support thinking, strip suffix and passthrough
        let mut body = body;
        body["model"] = serde_json::Value::String(base_model);
        return InjectionResult::PassThrough(body);
    }

    // 5. Resolve thinking config (map and clamp)
    let thinking_config = match resolve_thinking_config(thinking_value, model_info, protocol) {
        Ok(config) => config,
        Err(e) => return InjectionResult::Error(e),
    };

    // 6. Protocol-specific injection
    let injected = match protocol {
        Protocol::OpenAI => {
            let is_responses = is_responses_endpoint(request_path);
            inject_openai(body, &base_model, thinking_config, is_responses)
        }
        Protocol::Anthropic => inject_anthropic(body, &base_model, thinking_config, model_info),
        Protocol::Gemini => inject_gemini(body, &base_model, thinking_config),
    };

    InjectionResult::Injected(injected)
}

/// Resolve thinking configuration.
///
/// Converts input value (level or budget) to the format required by the target protocol,
/// applying appropriate mapping and clamping.
///
/// Key design: Return type is determined by **protocol requirements**, not input type or model's native format:
/// - OpenAI protocol: always returns Effort (or Disabled for "none")
/// - Anthropic protocol: always returns Budget (or Disabled for 0)
/// - Gemini protocol: depends on model (2.5 uses Budget, 3 uses Effort based on whether model has levels)
///
/// # Design Decision: Disabled Thinking
///
/// When `level = "none"` or `budget = 0`, returns `ThinkingConfig::Disabled`.
/// This is handled **before** any clamping, so `zero_allowed` does not affect this behavior.
/// The `zero_allowed` flag only affects whether `budget_tokens: 0` can be sent to the API
/// (for models that support it), but for disabling thinking we use protocol-specific methods.
fn resolve_thinking_config(
    thinking_value: ThinkingValue,
    model_info: &ModelInfo,
    protocol: Protocol,
) -> Result<ThinkingConfig, InjectionError> {
    let thinking = model_info.thinking.as_ref().unwrap();
    let model_uses_levels = thinking.levels.is_some();

    // Check if model has a valid budget range (for clamping)
    // If max == 0, model only uses Levels (like OpenAI models)
    let has_budget_range = thinking.max > 0;

    // Determine what return type the protocol needs
    let needs_effort = match protocol {
        Protocol::OpenAI => true,
        Protocol::Anthropic => false,
        Protocol::Gemini => model_uses_levels, // Gemini 2.5 no levels, Gemini 3 has levels
    };

    match thinking_value {
        ThinkingValue::Budget(budget) => {
            // Handle disabled thinking first (before any clamping)
            if budget == 0 {
                if matches!(protocol, Protocol::Gemini) {
                    // Gemini protocol: pass through 0 as Budget(0)
                    // Let upstream protocol converter handle disabling thinking
                    return Ok(ThinkingConfig::Budget(0));
                } else {
                    // OpenAI/Anthropic protocol: return Disabled
                    return Ok(ThinkingConfig::Disabled);
                }
            }

            // Numeric suffix
            // Determine if dynamic budget (-1) is allowed based on protocol AND model
            // - Anthropic protocol doesn't support budget_tokens: -1
            // - OpenAI protocol: -1 goes through budget_to_effort → "auto" → "medium" (handled separately)
            // - Gemini protocol: depends on model's dynamic_allowed
            let protocol_allows_dynamic = matches!(protocol, Protocol::Gemini);
            let effective_dynamic_allowed = thinking.dynamic_allowed && protocol_allows_dynamic;

            let clamped = if has_budget_range {
                // For Effort output, -1 should stay as -1 to become "auto" → "medium"
                // This ensures (-1) and (auto) have consistent semantics for OpenAI protocol
                if needs_effort && budget == -1 {
                    budget // Pass through -1, budget_to_effort(-1) → "auto" → "medium"
                } else {
                    clamp_budget(
                        budget,
                        thinking.min,
                        thinking.max,
                        thinking.zero_allowed,
                        effective_dynamic_allowed,
                        thinking.auto_budget,
                    )
                }
            } else {
                // Model has no budget range (like OpenAI models), use raw value
                budget
            };

            // For Gemini protocol with numeric suffix, respect user intent: use Budget directly
            // User explicitly provided a number, so output thinkingBudget, not thinkingLevel
            if matches!(protocol, Protocol::Gemini) {
                return Ok(ThinkingConfig::Budget(clamped));
            }

            if needs_effort {
                // Protocol needs Effort, convert budget to level using generic mapping
                let effort = budget_to_effort(clamped);
                let clamped_effort = if let Some(levels) = thinking.levels {
                    // Model has discrete levels, clamp to supported levels
                    clamp_effort_to_levels(effort, levels)
                } else {
                    effort
                };
                // OpenAI protocol doesn't support "auto", treat as "medium"
                // This applies regardless of whether model supports auto
                let final_effort = if clamped_effort == "auto" {
                    "medium"
                } else {
                    clamped_effort
                };
                Ok(ThinkingConfig::Effort(final_effort.to_string()))
            } else {
                // Protocol needs Budget
                Ok(ThinkingConfig::Budget(clamped))
            }
        }
        ThinkingValue::Level(level) => {
            // Level suffix
            let level = level.to_lowercase();

            // Handle disabled thinking first
            if level == "none" {
                if matches!(protocol, Protocol::Gemini) {
                    // Gemini protocol: pass through 0 as Budget(0)
                    // Let upstream protocol converter handle disabling thinking
                    return Ok(ThinkingConfig::Budget(0));
                } else {
                    // OpenAI/Anthropic protocol: return Disabled
                    return Ok(ThinkingConfig::Disabled);
                }
            }

            // Handle (auto) level for Gemini protocol: use Budget(-1) for dynamic thinking
            // This respects user intent: "auto" means dynamic, Gemini supports thinkingBudget: -1
            if level == "auto" && matches!(protocol, Protocol::Gemini) {
                return Ok(ThinkingConfig::Budget(-1));
            }

            // Validate level string
            if level_to_budget(&level).is_none() {
                return Err(InjectionError {
                    status: 400,
                    message: format!("invalid thinking level: {}", level),
                });
            }

            if needs_effort {
                // Protocol needs Effort
                let clamped_effort = if let Some(levels) = thinking.levels {
                    // Model has discrete levels, clamp to supported levels
                    clamp_effort_to_levels(&level, levels)
                } else {
                    &level
                };
                // OpenAI protocol doesn't support "auto", treat as "medium"
                // This applies regardless of whether model supports auto
                let final_effort = if clamped_effort == "auto" {
                    "medium"
                } else {
                    clamped_effort
                };
                Ok(ThinkingConfig::Effort(final_effort.to_string()))
            } else {
                // Protocol needs Budget, convert level to budget (already validated, unwrap safe)
                let budget = level_to_budget(&level).unwrap();

                // Determine if dynamic budget (-1) is allowed based on protocol AND model
                // (auto) → -1, Anthropic protocol doesn't support budget_tokens: -1
                let protocol_allows_dynamic = matches!(protocol, Protocol::Gemini);
                let effective_dynamic_allowed = thinking.dynamic_allowed && protocol_allows_dynamic;

                let clamped = if has_budget_range {
                    clamp_budget(
                        budget,
                        thinking.min,
                        thinking.max,
                        thinking.zero_allowed,
                        effective_dynamic_allowed,
                        thinking.auto_budget,
                    )
                } else {
                    // Model has no budget range (e.g., OpenAI model via Anthropic protocol)
                    // But still need to handle -1 for Anthropic protocol
                    if budget == -1 && !effective_dynamic_allowed {
                        thinking.auto_budget.unwrap_or(8192) // Default to medium if no auto_budget
                    } else {
                        budget
                    }
                };
                Ok(ThinkingConfig::Budget(clamped))
            }
        }
        ThinkingValue::None => unreachable!("None case handled earlier"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ===== Basic Functionality Tests =====

    #[test]
    fn test_inject_no_suffix_passthrough() {
        let body = json!({"model": "claude-sonnet-4", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::PassThrough(b) => {
                assert_eq!(b["model"], "claude-sonnet-4");
            }
            _ => panic!("Expected PassThrough"),
        }
    }

    #[test]
    fn test_inject_empty_parentheses_passthrough() {
        let body = json!({"model": "claude-sonnet-4()", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4()",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::PassThrough(b) => {
                assert_eq!(b["model"], "claude-sonnet-4");
            }
            _ => panic!("Expected PassThrough"),
        }
    }

    #[test]
    fn test_inject_unknown_model_error() {
        let body = json!({"model": "unknown-model(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "unknown-model(high)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Error(e) => {
                assert_eq!(e.status, 400);
                assert!(e.message.contains("unknown model"));
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_inject_known_model_no_thinking_passthrough() {
        let body = json!({"model": "claude-haiku-4-5-20251001(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-haiku-4-5-20251001(high)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::PassThrough(b) => {
                assert_eq!(b["model"], "claude-haiku-4-5-20251001");
            }
            _ => panic!("Expected PassThrough"),
        }
    }

    #[test]
    fn test_inject_invalid_level_error() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(superfast)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(superfast)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Error(e) => {
                assert_eq!(e.status, 400);
                assert!(e.message.contains("invalid thinking level"));
            }
            _ => panic!("Expected Error"),
        }
    }

    // ===== Claude Model Tests (Budget-based) =====

    // Claude + Anthropic Protocol (Native)
    #[test]
    fn test_claude_anthropic_none_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(none)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_claude_anthropic_auto_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(auto)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 16384); // auto_budget
            }
            _ => panic!("Expected Injected with auto budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_minimal_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(minimal)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 1024); // clamped to min
            }
            _ => panic!("Expected Injected with clamped minimal budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_low_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(low)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 1024); // exactly min
            }
            _ => panic!("Expected Injected with low budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_medium_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(medium)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 8192);
            }
            _ => panic!("Expected Injected with medium budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_high_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(high)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 24576);
            }
            _ => panic!("Expected Injected with high budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_xhigh_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(xhigh)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 32768);
            }
            _ => panic!("Expected Injected with xhigh budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_zero_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(0)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_claude_anthropic_negative_one_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(-1)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 16384); // auto_budget
            }
            _ => panic!("Expected Injected with auto budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_500_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(500)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(500)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 1024); // clamped to min
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_in_range_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(50000)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 50000); // within range
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_claude_anthropic_above_max_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(150000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(150000)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["thinking"]["budget_tokens"], 100000); // clamped to max
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    // Claude + OpenAI Protocol (Cross-protocol)
    #[test]
    fn test_claude_openai_none_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(none)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
        }
    }

    #[test]
    fn test_claude_openai_auto_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(auto)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "medium"); // auto → medium
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_claude_openai_minimal_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(minimal)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "minimal");
            }
            _ => panic!("Expected Injected with minimal effort"),
        }
    }

    #[test]
    fn test_claude_openai_zero_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(0)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
        }
    }

    #[test]
    fn test_claude_openai_negative_one_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(-1)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "medium"); // -1 → auto → medium
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_claude_openai_500_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(500)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(500)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "low"); // 500 → clamp to 1024 → low
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_claude_openai_512_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(512)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(512)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "low"); // 512 → clamp to 1024 → low
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_claude_openai_8192_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(8192)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_claude_openai_24576_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(24576)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(24576)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_claude_openai_32768_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(32768)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(32768)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "xhigh");
            }
            _ => panic!("Expected Injected with xhigh effort"),
        }
    }

    #[test]
    fn test_claude_openai_100000_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(100000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(100000)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["reasoning_effort"], "xhigh");
            }
            _ => panic!("Expected Injected with xhigh effort"),
        }
    }

    // Claude + Gemini Protocol (Cross-protocol)
    #[test]
    fn test_claude_gemini_none_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(none)",
            Protocol::Gemini,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_claude_gemini_zero_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(0)",
            Protocol::Gemini,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_claude_gemini_auto_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(auto)",
            Protocol::Gemini,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 16384); // auto_budget
            }
            _ => panic!("Expected Injected with auto budget"),
        }
    }

    #[test]
    fn test_claude_gemini_minimal_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(minimal)",
            Protocol::Gemini,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 1024); // clamped to min
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    #[test]
    fn test_claude_gemini_negative_one_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(-1)",
            Protocol::Gemini,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 16384); // auto_budget
            }
            _ => panic!("Expected Injected with auto budget"),
        }
    }

    #[test]
    fn test_claude_gemini_in_range_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(50000)",
            Protocol::Gemini,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 50000); // within range
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_claude_gemini_above_max_suffix() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(150000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(150000)",
            Protocol::Gemini,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 100000); // clamped to max
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    // ===== Gemini 2.5 Model Tests (Budget-based) =====

    // Gemini 2.5 + Gemini Protocol (Native)
    #[test]
    fn test_gemini_25_gemini_none_suffix() {
        let body = json!({"model": "gemini-2.5-pro(none)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(none)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_zero_suffix() {
        let body = json!({"model": "gemini-2.5-pro(0)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(0)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_auto_suffix() {
        let body = json!({"model": "gemini-2.5-pro(auto)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(auto)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1); // dynamic_allowed=true
            }
            _ => panic!("Expected Injected with dynamic budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_minimal_suffix() {
        let body = json!({"model": "gemini-2.5-pro(minimal)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(minimal)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 512);
            }
            _ => panic!("Expected Injected with minimal budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_low_suffix() {
        let body = json!({"model": "gemini-2.5-pro(low)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(low)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 1024);
            }
            _ => panic!("Expected Injected with low budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_medium_suffix() {
        let body = json!({"model": "gemini-2.5-pro(medium)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(medium)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 8192);
            }
            _ => panic!("Expected Injected with medium budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_high_suffix() {
        let body = json!({"model": "gemini-2.5-pro(high)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(high)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 24576);
            }
            _ => panic!("Expected Injected with high budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_xhigh_suffix() {
        let body = json!({"model": "gemini-2.5-pro(xhigh)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(xhigh)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 32768);
            }
            _ => panic!("Expected Injected with xhigh budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_negative_one_suffix() {
        let body = json!({"model": "gemini-2.5-pro(-1)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(-1)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1); // dynamic_allowed=true
            }
            _ => panic!("Expected Injected with dynamic budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_50_suffix() {
        let body = json!({"model": "gemini-2.5-pro(50)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(50)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 128); // clamped to min
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_in_range_suffix() {
        let body = json!({"model": "gemini-2.5-pro(16384)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(16384)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 16384); // within range
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_gemini_25_gemini_above_max_suffix() {
        let body = json!({"model": "gemini-2.5-pro(50000)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(50000)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 32768); // clamped to max
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    // Gemini 2.5 + OpenAI Protocol (Cross-protocol)
    #[test]
    fn test_gemini_25_openai_none_suffix() {
        let body = json!({"model": "gemini-2.5-pro(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(none)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_auto_suffix() {
        let body = json!({"model": "gemini-2.5-pro(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(auto)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "medium"); // auto → medium
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_minimal_suffix() {
        let body = json!({"model": "gemini-2.5-pro(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(minimal)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "minimal");
            }
            _ => panic!("Expected Injected with minimal effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_zero_suffix() {
        let body = json!({"model": "gemini-2.5-pro(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(0)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_negative_one_suffix() {
        let body = json!({"model": "gemini-2.5-pro(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(-1)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "medium"); // -1 → auto → medium
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_50_suffix() {
        let body = json!({"model": "gemini-2.5-pro(50)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(50)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "minimal"); // 50 → clamp to 128 → minimal
            }
            _ => panic!("Expected Injected with minimal effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_512_suffix() {
        let body = json!({"model": "gemini-2.5-pro(512)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(512)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "minimal");
            }
            _ => panic!("Expected Injected with minimal effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_8192_suffix() {
        let body = json!({"model": "gemini-2.5-pro(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(8192)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_24576_suffix() {
        let body = json!({"model": "gemini-2.5-pro(24576)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(24576)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_32768_suffix() {
        let body = json!({"model": "gemini-2.5-pro(32768)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(32768)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "xhigh");
            }
            _ => panic!("Expected Injected with xhigh effort"),
        }
    }

    #[test]
    fn test_gemini_25_openai_50000_suffix() {
        let body = json!({"model": "gemini-2.5-pro(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(50000)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["reasoning_effort"], "xhigh"); // 50000 → clamp to 32768 → xhigh
            }
            _ => panic!("Expected Injected with xhigh effort"),
        }
    }

    // Gemini 2.5 + Anthropic Protocol (Cross-protocol)
    #[test]
    fn test_gemini_25_anthropic_none_suffix() {
        let body = json!({"model": "gemini-2.5-pro(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(none)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_gemini_25_anthropic_auto_suffix() {
        let body = json!({"model": "gemini-2.5-pro(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(auto)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["budget_tokens"], 16448); // (128+32768)/2
            }
            _ => panic!("Expected Injected with calculated auto budget"),
        }
    }

    #[test]
    fn test_gemini_25_anthropic_minimal_suffix() {
        let body = json!({"model": "gemini-2.5-pro(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(minimal)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["budget_tokens"], 512);
            }
            _ => panic!("Expected Injected with minimal budget"),
        }
    }

    #[test]
    fn test_gemini_25_anthropic_zero_suffix() {
        let body = json!({"model": "gemini-2.5-pro(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(0)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_gemini_25_anthropic_negative_one_suffix() {
        let body = json!({"model": "gemini-2.5-pro(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(-1)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["budget_tokens"], 16448); // (128+32768)/2
            }
            _ => panic!("Expected Injected with calculated auto budget"),
        }
    }

    #[test]
    fn test_gemini_25_anthropic_50_suffix() {
        let body = json!({"model": "gemini-2.5-pro(50)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(50)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["budget_tokens"], 128); // clamped to min
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    #[test]
    fn test_gemini_25_anthropic_in_range_suffix() {
        let body = json!({"model": "gemini-2.5-pro(16384)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(16384)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["budget_tokens"], 16384); // within range
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_gemini_25_anthropic_50000_suffix() {
        let body = json!({"model": "gemini-2.5-pro(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-2.5-pro(50000)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["thinking"]["budget_tokens"], 32768); // clamped to max
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    // ===== OpenAI Model Tests (Level-based, no budget range) =====

    // OpenAI + OpenAI Protocol (Native)
    #[test]
    fn test_openai_openai_none_suffix() {
        let body = json!({"model": "gpt-5.1(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(none)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
        }
    }

    #[test]
    fn test_openai_openai_auto_suffix() {
        let body = json!({"model": "gpt-5.1(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(auto)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "medium"); // auto → medium
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_openai_openai_minimal_suffix() {
        let body = json!({"model": "gpt-5.1(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(minimal)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "low"); // minimal not in levels → clamp up to low
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_openai_openai_low_suffix() {
        let body = json!({"model": "gpt-5.1(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(low)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "low");
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_openai_openai_medium_suffix() {
        let body = json!({"model": "gpt-5.1(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(medium)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_openai_openai_high_suffix() {
        let body = json!({"model": "gpt-5.1(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(high)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_openai_openai_xhigh_suffix() {
        let body = json!({"model": "gpt-5.1(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(xhigh)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "high"); // xhigh not in levels → clamp to highest (high)
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_openai_openai_zero_suffix() {
        let body = json!({"model": "gpt-5.1(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(0)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
        }
    }

    #[test]
    fn test_openai_openai_negative_one_suffix() {
        let body = json!({"model": "gpt-5.1(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(-1)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "medium"); // -1 → auto → medium
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_openai_openai_8192_suffix() {
        let body = json!({"model": "gpt-5.1(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(8192)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with medium effort"),
        }
    }

    #[test]
    fn test_openai_openai_50000_suffix() {
        let body = json!({"model": "gpt-5.1(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(50000)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["reasoning_effort"], "high"); // 50000 → xhigh → clamp to high
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    // OpenAI + Anthropic Protocol (Cross-protocol)
    #[test]
    fn test_openai_anthropic_none_suffix() {
        let body = json!({"model": "gpt-5.1(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(none)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_openai_anthropic_auto_suffix() {
        let body = json!({"model": "gpt-5.1(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(auto)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 8192); // default
            }
            _ => panic!("Expected Injected with default budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_minimal_suffix() {
        let body = json!({"model": "gpt-5.1(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(minimal)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 512);
            }
            _ => panic!("Expected Injected with minimal budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_low_suffix() {
        let body = json!({"model": "gpt-5.1(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(low)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 1024);
            }
            _ => panic!("Expected Injected with low budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_medium_suffix() {
        let body = json!({"model": "gpt-5.1(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(medium)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 8192);
            }
            _ => panic!("Expected Injected with medium budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_high_suffix() {
        let body = json!({"model": "gpt-5.1(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(high)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 24576);
            }
            _ => panic!("Expected Injected with high budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_xhigh_suffix() {
        let body = json!({"model": "gpt-5.1(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(xhigh)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 32768);
            }
            _ => panic!("Expected Injected with xhigh budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_zero_suffix() {
        let body = json!({"model": "gpt-5.1(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(0)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_openai_anthropic_negative_one_suffix() {
        let body = json!({"model": "gpt-5.1(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(-1)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 8192); // default
            }
            _ => panic!("Expected Injected with default budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_8192_suffix() {
        let body = json!({"model": "gpt-5.1(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(8192)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 8192); // no clamp (no range)
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_openai_anthropic_50000_suffix() {
        let body = json!({"model": "gpt-5.1(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(50000)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["thinking"]["budget_tokens"], 50000); // no clamp (no range)
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    // OpenAI + Gemini Protocol (Cross-protocol)
    #[test]
    fn test_openai_gemini_none_suffix() {
        let body = json!({"model": "gpt-5.1(none)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(none)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_openai_gemini_zero_suffix() {
        let body = json!({"model": "gpt-5.1(0)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(0)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_openai_gemini_auto_suffix() {
        let body = json!({"model": "gpt-5.1(auto)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(auto)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1); // Gemini special handling
            }
            _ => panic!("Expected Injected with dynamic budget"),
        }
    }

    #[test]
    fn test_openai_gemini_minimal_suffix() {
        let body = json!({"model": "gpt-5.1(minimal)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(minimal)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low"); // clamp up
            }
            _ => panic!("Expected Injected with low level"),
        }
    }

    #[test]
    fn test_openai_gemini_low_suffix() {
        let body = json!({"model": "gpt-5.1(low)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(low)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low");
            }
            _ => panic!("Expected Injected with low level"),
        }
    }

    #[test]
    fn test_openai_gemini_medium_suffix() {
        let body = json!({"model": "gpt-5.1(medium)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(medium)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "medium");
            }
            _ => panic!("Expected Injected with medium level"),
        }
    }

    #[test]
    fn test_openai_gemini_high_suffix() {
        let body = json!({"model": "gpt-5.1(high)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(high)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high");
            }
            _ => panic!("Expected Injected with high level"),
        }
    }

    #[test]
    fn test_openai_gemini_xhigh_suffix() {
        let body = json!({"model": "gpt-5.1(xhigh)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(xhigh)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high"); // clamp to highest
            }
            _ => panic!("Expected Injected with high level"),
        }
    }

    #[test]
    fn test_openai_gemini_negative_one_suffix() {
        let body = json!({"model": "gpt-5.1(-1)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(-1)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1);
            }
            _ => panic!("Expected Injected with dynamic budget"),
        }
    }

    #[test]
    fn test_openai_gemini_8192_suffix() {
        let body = json!({"model": "gpt-5.1(8192)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(8192)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 8192);
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_openai_gemini_50000_suffix() {
        let body = json!({"model": "gpt-5.1(50000)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gpt-5.1(50000)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gpt-5.1");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 50000);
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    // ===== Gemini 3 Model Tests (Level-based, with budget range) =====

    // Gemini 3 + Gemini Protocol (Native)
    #[test]
    fn test_gemini_3_gemini_none_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(none)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(none)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_zero_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(0)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(0)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with budget 0"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_auto_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(auto)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(auto)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1); // Gemini special handling
            }
            _ => panic!("Expected Injected with dynamic budget"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_minimal_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(minimal)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(minimal)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low"); // clamp up
            }
            _ => panic!("Expected Injected with low level"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_low_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(low)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(low)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low");
            }
            _ => panic!("Expected Injected with low level"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_medium_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(medium)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(medium)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high"); // not in list → clamp up
            }
            _ => panic!("Expected Injected with high level"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_high_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(high)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(high)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high");
            }
            _ => panic!("Expected Injected with high level"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_xhigh_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(xhigh)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(xhigh)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high"); // clamp to highest
            }
            _ => panic!("Expected Injected with high level"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_negative_one_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(-1)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(-1)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1); // dynamic_allowed=true
            }
            _ => panic!("Expected Injected with dynamic budget"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_50_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(50)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(50)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 128); // clamped to min
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_500_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(500)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(500)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 500); // within range
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_1024_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(1024)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(1024)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 1024);
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_8192_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(8192)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(8192)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 8192);
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_24576_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(24576)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(24576)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 24576);
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_gemini_3_gemini_50000_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(50000)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(50000)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 32768); // clamped to max
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    // Gemini 3 + OpenAI Protocol (Cross-protocol)
    #[test]
    fn test_gemini_3_openai_none_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(none)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "low"); // none not in levels → clamp up to low
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_auto_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(auto)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "high"); // auto → medium → clamp to high
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_minimal_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(minimal)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "low"); // clamp up
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_low_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(low)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "low");
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_medium_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(medium)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "high"); // clamp to high
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_high_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(high)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_xhigh_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(xhigh)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "high"); // clamp to highest
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_zero_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(0)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "low"); // 0 → none → clamp to low
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_negative_one_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(-1)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "high"); // -1 → auto → medium → clamp to high
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_500_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(500)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(500)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "low"); // 500 → minimal → clamp to low
            }
            _ => panic!("Expected Injected with low effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_8192_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(8192)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "high"); // 8192 → medium → clamp to high
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    #[test]
    fn test_gemini_3_openai_50000_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(50000)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "high"); // 50000 → clamp to 32768 → xhigh → clamp to high
            }
            _ => panic!("Expected Injected with high effort"),
        }
    }

    // Gemini 3 + Anthropic Protocol (Cross-protocol)
    #[test]
    fn test_gemini_3_anthropic_none_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(none)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_auto_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(auto)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 16448); // (128+32768)/2
            }
            _ => panic!("Expected Injected with calculated auto budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_minimal_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(minimal)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 512);
            }
            _ => panic!("Expected Injected with minimal budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_low_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(low)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 1024);
            }
            _ => panic!("Expected Injected with low budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_medium_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(medium)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 8192);
            }
            _ => panic!("Expected Injected with medium budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_high_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(high)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 24576);
            }
            _ => panic!("Expected Injected with high budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_xhigh_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(xhigh)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 32768);
            }
            _ => panic!("Expected Injected with xhigh budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_zero_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(0)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with disabled thinking"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_negative_one_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(-1)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 16448); // (128+32768)/2
            }
            _ => panic!("Expected Injected with calculated auto budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_50_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(50)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(50)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 128); // clamped to min
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_16384_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(16384)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(16384)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 16384); // within range
            }
            _ => panic!("Expected Injected with exact budget"),
        }
    }

    #[test]
    fn test_gemini_3_anthropic_50000_suffix() {
        let body = json!({"model": "gemini-3-pro-preview(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-pro-preview(50000)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 32768); // clamped to max
            }
            _ => panic!("Expected Injected with clamped budget"),
        }
    }
}
