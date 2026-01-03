//! Unified thinking configuration injection.
//!
//! This module coordinates the entire thinking injection workflow:
//! parsing, validation, mapping, clamping, and protocol-specific injection.

use crate::models::registry::{get_model_info, ModelInfo};
use crate::protocol::{inject_anthropic, inject_gemini, inject_openai, Protocol};
use crate::thinking::models::{budget_to_effort, clamp_budget, clamp_effort_to_levels, level_to_budget};
use crate::thinking::parser::{parse_model_suffix, ThinkingValue};
use crate::thinking::ThinkingConfig;

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
            })
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
                let final_effort = if clamped_effort == "auto" { "medium" } else { clamped_effort };
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
                let final_effort = if clamped_effort == "auto" { "medium" } else { clamped_effort };
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

    // ===== 1. Claude Model Tests =====

    // 1.1 Claude + Anthropic Protocol Tests
    #[test]
    fn test_claude_anthropic_none() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(none)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_auto() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(auto)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["type"], "enabled");
                assert_eq!(b["thinking"]["budget_tokens"], 16384); // auto_budget
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_minimal() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(minimal)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 1024); // clamped to min
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_low() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(low)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 1024);
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_medium() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(medium)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 8192);
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_high() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(high)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["type"], "enabled");
                assert_eq!(b["thinking"]["budget_tokens"], 24576);
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_xhigh() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(xhigh)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 32768);
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_zero() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(0)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_negative_one() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(-1)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 16384); // auto_budget
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_500() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(500)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(500)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 1024); // clamped to min
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_16384() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(16384)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(16384)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 16384);
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_anthropic_150000() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(150000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(150000)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["thinking"]["budget_tokens"], 100000); // clamped to max
            }
            _ => panic!("Expected Injected"),
        }
    }

    // 1.2 Claude + OpenAI Protocol Tests
    #[test]
    fn test_claude_openai_none() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(none)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_auto() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(auto)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "medium"); // OpenAI doesn't support auto
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_minimal() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(minimal)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "minimal");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_low() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(low)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "low");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_medium() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(medium)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_high() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(high)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_xhigh() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(xhigh)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "xhigh");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_zero() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(0)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_negative_one() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(-1)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "medium"); // -1 → auto → medium
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_500() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(500)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(500)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "low"); // 500 → clamped to 1024 → low
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_512() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(512)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(512)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "low"); // 512 → clamped to 1024 → low
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_8192() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(8192)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_24576() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(24576)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(24576)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_32768() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(32768)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(32768)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "xhigh");
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_claude_openai_100000() {
        let body = json!({"model": "claude-sonnet-4-5-20250929(100000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "claude-sonnet-4-5-20250929(100000)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(b) => {
                assert_eq!(b["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(b["reasoning_effort"], "xhigh");
            }
            _ => panic!("Expected Injected"),
        }
    }
