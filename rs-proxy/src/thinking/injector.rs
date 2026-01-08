//! Unified thinking configuration injection.
//!
//! This module coordinates the entire thinking injection workflow:
//! parsing, validation, mapping, clamping, and protocol-specific injection.

use crate::models::registry::{ModelInfo, get_model_info};
use crate::protocol::{Protocol, inject_anthropic, inject_gemini, inject_openai};
use crate::thinking::{FixedThinking, ThinkingConfig, ThinkingIntent};
use crate::thinking::models::{
    budget_to_effort, clamp_budget, clamp_effort_to_levels, level_to_budget,
};
use crate::thinking::parser::parse_model_suffix;

/// Default budget for "medium" level, used when auto_budget is not configured.
const DEFAULT_MEDIUM_BUDGET: i32 = 8192; // level_to_budget("medium")

/// Native Gemini model prefixes (whitelist).
///
/// These are models that run natively on Gemini API.
/// Models like `gemini-claude-*` are cross-protocol (Claude via Gemini API)
/// and should NOT be treated as native Gemini models.
const NATIVE_GEMINI_PREFIXES: &[&str] = &[
    "gemini-2.5-",
    "gemini-3-",
    "gemini-pro",
    "gemini-flash",
];

/// Check if model is a native Gemini model.
fn is_native_gemini_model(model_id: &str) -> bool {
    NATIVE_GEMINI_PREFIXES.iter().any(|prefix| model_id.starts_with(prefix))
}

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
/// 2. Convert to intent (Disabled/Dynamic/Fixed)
/// 3. Validate model exists in registry
/// 4. Check thinking support
/// 5. Resolve intent to protocol-specific config
/// 6. Protocol-specific injection
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

    // 2. Convert to intent - early return if no suffix
    let intent = match parsed.thinking.to_intent() {
        None => {
            // No suffix: update model name and passthrough
            let mut body = body;
            body["model"] = serde_json::Value::String(base_model);
            return InjectionResult::PassThrough(body);
        }
        Some(intent) => intent,
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

    // 5. Resolve intent to thinking config (with protocol adaptation)
    let thinking_config = match resolve_intent_to_config(intent, model_info, protocol) {
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

/// Resolve user intent to protocol-specific thinking configuration.
///
/// This function handles three distinct intents:
/// - `Disabled`: User wants to disable thinking
/// - `Dynamic`: User wants dynamic/auto thinking
/// - `Fixed`: User specified a concrete level or budget
///
/// The output format is determined by **protocol requirements**:
/// - OpenAI protocol: always returns Effort (or Disabled)
/// - Anthropic protocol: always returns Budget (or Disabled)
/// - Gemini protocol: depends on model (3.x uses Effort for levels, 2.5 uses Budget)
///
/// # Intent Processing
///
/// ## Disabled Intent
/// - Anthropic: `Disabled` → `thinking: { type: "disabled" }`
/// - OpenAI: `Disabled` → `reasoning_effort: "none"`
/// - Gemini (with levels): `Budget(0)` → `thinkingBudget: 0`
/// - Gemini 2.5 (native): clamp to min (API doesn't support 0)
/// - Gemini (cross-protocol): `Budget(0)` → `thinkingBudget: 0`
///
/// ## Dynamic Intent
/// - Anthropic: use `auto_budget` or `(min+max)/2` (API doesn't support -1)
/// - OpenAI: `Effort("medium")` (API doesn't support auto)
/// - Gemini: `Budget(-1)` (API supports dynamic)
///
/// ## Fixed Intent
/// - Apply clamping and convert to protocol format
fn resolve_intent_to_config(
    intent: ThinkingIntent,
    model_info: &ModelInfo,
    protocol: Protocol,
) -> Result<ThinkingConfig, InjectionError> {
    let thinking = model_info.thinking.as_ref().unwrap();
    let model_uses_levels = thinking.levels.is_some();

    // Determine what return type the protocol needs
    // For Gemini protocol with level input, use Effort for models with levels
    // For Gemini protocol with budget input, use Budget to respect user intent
    let needs_effort = match protocol {
        Protocol::OpenAI => true,
        Protocol::Anthropic => false,
        Protocol::Gemini => model_uses_levels,
    };

    match intent {
        // ===== DISABLED INTENT =====
        // User requested (none) or (0) - wants to disable thinking
        ThinkingIntent::Disabled => match protocol {
            Protocol::Anthropic | Protocol::OpenAI => Ok(ThinkingConfig::Disabled),
            Protocol::Gemini => {
                // Gemini protocol: behavior depends on model type
                if model_uses_levels {
                    // Gemini 3 (has levels): use Budget(0) to disable
                    Ok(ThinkingConfig::Budget(0))
                } else if is_native_gemini_model(model_info.id) {
                    // Gemini 2.5 (native, no levels): clamp to min
                    Ok(ThinkingConfig::Budget(thinking.min))
                } else {
                    // Cross-protocol (e.g., Claude via Gemini API): pass through 0
                    Ok(ThinkingConfig::Budget(0))
                }
            }
        },

        // ===== DYNAMIC INTENT =====
        // User requested (auto) or (-1) - wants dynamic/auto thinking
        ThinkingIntent::Dynamic => match protocol {
            Protocol::Anthropic => {
                // Anthropic doesn't support budget_tokens: -1
                // Use auto_budget if configured, otherwise (min+max)/2
                let fallback = thinking.auto_budget.unwrap_or_else(|| {
                    if thinking.max > 0 {
                        (thinking.min + thinking.max) / 2
                    } else {
                        DEFAULT_MEDIUM_BUDGET
                    }
                });
                Ok(ThinkingConfig::Budget(fallback))
            }
            Protocol::OpenAI => {
                // OpenAI doesn't support reasoning_effort: "auto"
                // Convert to "medium", then clamp to model's supported levels
                let effort = if let Some(levels) = thinking.levels {
                    clamp_effort_to_levels("medium", levels)
                } else {
                    "medium"
                };
                Ok(ThinkingConfig::Effort(effort.to_string()))
            }
            Protocol::Gemini => {
                // Gemini supports thinkingBudget: -1 for dynamic
                Ok(ThinkingConfig::Budget(-1))
            }
        },

        // ===== FIXED INTENT =====
        // User specified a concrete level or budget
        ThinkingIntent::Fixed(fixed) => match fixed {
            FixedThinking::Level(level) => {
                resolve_fixed_level(&level, model_info, protocol, needs_effort)
            }
            FixedThinking::Budget(budget) => {
                resolve_fixed_budget(budget, model_info, protocol, needs_effort)
            }
        },
    }
}

/// Resolve a fixed level to protocol-specific config.
fn resolve_fixed_level(
    level: &str,
    model_info: &ModelInfo,
    _protocol: Protocol,
    needs_effort: bool,
) -> Result<ThinkingConfig, InjectionError> {
    let thinking = model_info.thinking.as_ref().unwrap();

    // Validate level string
    let budget = match level_to_budget(level) {
        Some(b) => b,
        None => {
            return Err(InjectionError {
                status: 400,
                message: format!("invalid thinking level: {}", level),
            });
        }
    };
    
    if needs_effort {
        // Protocol needs Effort (OpenAI or Gemini 3 with levels)
        let effort = if let Some(levels) = thinking.levels {
            clamp_effort_to_levels(level, levels)
        } else {
            level
        };
        Ok(ThinkingConfig::Effort(effort.to_string()))
    } else {
        // Protocol needs Budget (Anthropic or Gemini 2.5)
        let clamped = clamp_budget_for_model(budget, thinking, false);
        Ok(ThinkingConfig::Budget(clamped))
    }
}

/// Resolve a fixed budget to protocol-specific config.
fn resolve_fixed_budget(
    budget: i32,
    model_info: &ModelInfo,
    protocol: Protocol,
    _needs_effort: bool,
) -> Result<ThinkingConfig, InjectionError> {
    let thinking = model_info.thinking.as_ref().unwrap();

    match protocol {
        Protocol::Gemini => {
            // Gemini protocol: respect user's budget input, use thinkingBudget
            let clamped = clamp_budget_for_model(budget, thinking, thinking.dynamic_allowed);
            Ok(ThinkingConfig::Budget(clamped))
        }
        Protocol::OpenAI => {
            // OpenAI protocol: convert budget to effort
            let clamped = clamp_budget_for_model(budget, thinking, false);
            let effort = budget_to_effort(clamped);
            let effort = if let Some(levels) = thinking.levels {
                clamp_effort_to_levels(effort, levels)
            } else {
                effort
            };
            Ok(ThinkingConfig::Effort(effort.to_string()))
        }
        Protocol::Anthropic => {
            // Anthropic protocol: use budget directly
            let clamped = clamp_budget_for_model(budget, thinking, false);
            Ok(ThinkingConfig::Budget(clamped))
        }
    }
}

/// Helper: Clamp budget using model's thinking support configuration.
///
/// Returns the budget unchanged if model has no budget range (max == 0).
fn clamp_budget_for_model(
    budget: i32,
    thinking: &crate::models::registry::ThinkingSupport,
    allow_dynamic: bool,
) -> i32 {
    if thinking.max > 0 {
        clamp_budget(
            budget,
            thinking.min,
            thinking.max,
            thinking.zero_allowed,
            allow_dynamic,
            thinking.auto_budget,
        )
    } else {
        budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ===== Native Gemini Model Detection Tests =====

    #[test]
    fn test_is_native_gemini_model() {
        // Native Gemini models (should return true)
        assert!(is_native_gemini_model("gemini-2.5-pro"));
        assert!(is_native_gemini_model("gemini-2.5-flash"));
        assert!(is_native_gemini_model("gemini-2.5-flash-lite"));
        assert!(is_native_gemini_model("gemini-3-pro-preview"));
        assert!(is_native_gemini_model("gemini-3-flash-preview"));
        assert!(is_native_gemini_model("gemini-pro-latest"));
        assert!(is_native_gemini_model("gemini-flash-latest"));

        // Cross-protocol models (should return false)
        assert!(!is_native_gemini_model("gemini-claude-opus-4-5-thinking"));
        assert!(!is_native_gemini_model("gemini-claude-sonnet-4-5-thinking"));

        // Non-Gemini models (should return false)
        assert!(!is_native_gemini_model("claude-sonnet-4"));
        assert!(!is_native_gemini_model("gpt-5.1"));
        assert!(!is_native_gemini_model("qwen-3-235b"));
    }

    // ===== Cross-Protocol Model Tests (gemini-claude-*) =====

    #[test]
    fn test_gemini_claude_gemini_auto_suffix() {
        // gemini-claude model with (auto) via Gemini protocol should get Budget(-1)
        let body = json!({"model": "gemini-claude-opus-4-5-thinking(auto)", "contents": []});
        let result = inject_thinking_config(
            body,
            "gemini-claude-opus-4-5-thinking(auto)",
            Protocol::Gemini,
            "/v1beta/models/gemini-claude-opus-4-5-thinking:generateContent",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-claude-opus-4-5-thinking");
                // Gemini protocol supports dynamic, so -1 passes through
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1);
            }
            _ => panic!("Expected Injected"),
        }
    }

    #[test]
    fn test_gemini_claude_anthropic_auto_suffix() {
        // gemini-claude model with (auto) via Anthropic protocol should get auto_budget=16384
        let body = json!({"model": "gemini-claude-opus-4-5-thinking(auto)", "messages": []});
        let result = inject_thinking_config(
            body,
            "gemini-claude-opus-4-5-thinking(auto)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-claude-opus-4-5-thinking");
                assert_eq!(body["thinking"]["type"], "enabled");
                assert_eq!(body["thinking"]["budget_tokens"], 16384);
            }
            _ => panic!("Expected Injected with budget 16384"),
        }
    }

    #[test]
    fn test_gemini_claude_gemini_none_suffix() {
        // gemini-claude model with (none) via Gemini protocol should get Budget(0)
        // because it's cross-protocol, not native Gemini
        let body = json!({"model": "gemini-claude-opus-4-5-thinking(none)", "contents": []});
        let result = inject_thinking_config(
            body,
            "gemini-claude-opus-4-5-thinking(none)",
            Protocol::Gemini,
            "/v1beta/models/gemini-claude-opus-4-5-thinking:generateContent",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-claude-opus-4-5-thinking");
                // Cross-protocol: Budget(0) passes through, NOT clamped to min
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with Budget(0)"),
        }
    }

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
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1); // Gemini uses -1 for dynamic thinking
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

        // Gemini protocol supports thinkingBudget: -1 for dynamic thinking
        // So -1 is passed through directly, not converted to auto_budget
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "claude-sonnet-4-5-20250929");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1);
            }
            _ => panic!("Expected Injected with dynamic budget"),
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

        // Gemini 2.5 has no levels and zero_allowed=false, so (none) clamps to min=128
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 128);
            }
            _ => panic!("Expected Injected with budget 128"),
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

        // Gemini 2.5 has no levels and zero_allowed=false, so (0) clamps to min=128
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-2.5-pro");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 128);
            }
            _ => panic!("Expected Injected with budget 128"),
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

        // Gemini 3 + OpenAI: (none) returns Disabled → reasoning_effort: "none"
        // Let upstream API decide how to handle it
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
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

        // Gemini 3 + OpenAI: (0) returns Disabled → reasoning_effort: "none"
        // Let upstream API decide how to handle it
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-pro-preview");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with none effort"),
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

    // ===== Gemini 3 Flash + Gemini Protocol Tests =====
    // Model: gemini-3-flash-preview
    // levels: ["minimal", "low", "medium", "high"], min: 128, max: 32768, dynamic_allowed: true

    #[test]
    fn test_gemini_3_flash_gemini_none_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(none)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(none)",
            Protocol::Gemini,
            "/v1/models",
        );

        // Gemini 3 has levels, so (none) returns Budget(0)
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with thinkingBudget: 0"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_zero_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(0)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(0)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
            }
            _ => panic!("Expected Injected with thinkingBudget: 0"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_auto_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(auto)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(auto)",
            Protocol::Gemini,
            "/v1/models",
        );

        // Gemini protocol + (auto) → thinkingBudget: -1
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1);
            }
            _ => panic!("Expected Injected with thinkingBudget: -1"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_negative_one_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(-1)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(-1)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1);
            }
            _ => panic!("Expected Injected with thinkingBudget: -1"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_minimal_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(minimal)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(minimal)",
            Protocol::Gemini,
            "/v1/models",
        );

        // Model has levels including "minimal" → thinkingLevel
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "minimal");
            }
            _ => panic!("Expected Injected with thinkingLevel: minimal"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_low_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(low)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(low)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low");
            }
            _ => panic!("Expected Injected with thinkingLevel: low"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_medium_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(medium)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(medium)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "medium");
            }
            _ => panic!("Expected Injected with thinkingLevel: medium"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_high_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(high)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(high)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high");
            }
            _ => panic!("Expected Injected with thinkingLevel: high"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_xhigh_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(xhigh)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(xhigh)",
            Protocol::Gemini,
            "/v1/models",
        );

        // levels don't include "xhigh" → clamp to "high"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high");
            }
            _ => panic!("Expected Injected with thinkingLevel: high (clamped from xhigh)"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_50_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(50)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(50)",
            Protocol::Gemini,
            "/v1/models",
        );

        // Numeric suffix → thinkingBudget, clamp to min (128)
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 128);
            }
            _ => panic!("Expected Injected with thinkingBudget: 128 (clamped)"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_512_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(512)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(512)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 512);
            }
            _ => panic!("Expected Injected with thinkingBudget: 512"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_8192_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(8192)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(8192)",
            Protocol::Gemini,
            "/v1/models",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 8192);
            }
            _ => panic!("Expected Injected with thinkingBudget: 8192"),
        }
    }

    #[test]
    fn test_gemini_3_flash_gemini_50000_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(50000)", "contents": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(50000)",
            Protocol::Gemini,
            "/v1/models",
        );

        // clamp to max (32768)
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 32768);
            }
            _ => panic!("Expected Injected with thinkingBudget: 32768 (clamped)"),
        }
    }

    // ===== Gemini 3 Flash + OpenAI Protocol Tests =====
    // Cross-protocol: Gemini model via OpenAI protocol

    #[test]
    fn test_gemini_3_flash_openai_none_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(none)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with reasoning_effort: none"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_zero_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(0)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "none");
            }
            _ => panic!("Expected Injected with reasoning_effort: none"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_auto_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(auto)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // OpenAI doesn't support "auto" → "medium"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with reasoning_effort: medium (auto → medium)"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_negative_one_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(-1)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // -1 → "auto" → "medium"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with reasoning_effort: medium (-1 → auto → medium)"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_minimal_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(minimal)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // "minimal" is in levels → pass through
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "minimal");
            }
            _ => panic!("Expected Injected with reasoning_effort: minimal"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_low_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(low)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "low");
            }
            _ => panic!("Expected Injected with reasoning_effort: low"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_medium_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(medium)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with reasoning_effort: medium"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_high_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(high)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with reasoning_effort: high"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_xhigh_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(xhigh)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // "xhigh" not in levels → clamp to "high"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with reasoning_effort: high (clamped from xhigh)"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_50_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(50)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(50)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // 50 → clamp to 128 → budget_to_effort(128) → "minimal"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "minimal");
            }
            _ => panic!("Expected Injected with reasoning_effort: minimal (50 → 128 → minimal)"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_512_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(512)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(512)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // 512 → budget_to_effort(512) → "minimal"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "minimal");
            }
            _ => panic!("Expected Injected with reasoning_effort: minimal"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_1024_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(1024)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(1024)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // 1024 → budget_to_effort(1024) → "low"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "low");
            }
            _ => panic!("Expected Injected with reasoning_effort: low"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_8192_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(8192)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // 8192 → budget_to_effort(8192) → "medium"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "medium");
            }
            _ => panic!("Expected Injected with reasoning_effort: medium"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_24576_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(24576)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(24576)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // 24576 → budget_to_effort(24576) → "high"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with reasoning_effort: high"),
        }
    }

    #[test]
    fn test_gemini_3_flash_openai_50000_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(50000)",
            Protocol::OpenAI,
            "/v1/chat/completions",
        );

        // 50000 → clamp to 32768 → budget_to_effort(32768) → "xhigh" → clamp to "high"
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["reasoning_effort"], "high");
            }
            _ => panic!("Expected Injected with reasoning_effort: high (50000 → 32768 → xhigh → high)"),
        }
    }

    // ===== Gemini 3 Flash + Anthropic Protocol Tests =====
    // Cross-protocol: Gemini model via Anthropic protocol

    #[test]
    fn test_gemini_3_flash_anthropic_none_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(none)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(none)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with thinking: disabled"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_zero_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(0)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(0)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["type"], "disabled");
            }
            _ => panic!("Expected Injected with thinking: disabled"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_auto_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(auto)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(auto)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        // Anthropic doesn't support -1 → use (min+max)/2 = (128+32768)/2 = 16448
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 16448);
            }
            _ => panic!("Expected Injected with budget_tokens: 16448"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_negative_one_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(-1)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(-1)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        // Anthropic doesn't support -1 → use (min+max)/2 = 16448
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 16448);
            }
            _ => panic!("Expected Injected with budget_tokens: 16448"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_minimal_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(minimal)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(minimal)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        // level_to_budget("minimal") → 512
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 512);
            }
            _ => panic!("Expected Injected with budget_tokens: 512"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_low_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(low)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(low)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 1024);
            }
            _ => panic!("Expected Injected with budget_tokens: 1024"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_medium_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(medium)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(medium)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 8192);
            }
            _ => panic!("Expected Injected with budget_tokens: 8192"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_high_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(high)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(high)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 24576);
            }
            _ => panic!("Expected Injected with budget_tokens: 24576"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_xhigh_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(xhigh)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(xhigh)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        // level_to_budget("xhigh") → 32768
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 32768);
            }
            _ => panic!("Expected Injected with budget_tokens: 32768"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_50_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(50)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(50)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        // clamp to min (128)
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 128);
            }
            _ => panic!("Expected Injected with budget_tokens: 128 (clamped)"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_512_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(512)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(512)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 512);
            }
            _ => panic!("Expected Injected with budget_tokens: 512"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_8192_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(8192)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(8192)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 8192);
            }
            _ => panic!("Expected Injected with budget_tokens: 8192"),
        }
    }

    #[test]
    fn test_gemini_3_flash_anthropic_50000_suffix() {
        let body = json!({"model": "gemini-3-flash-preview(50000)", "messages": []});
        let result = inject_thinking_config(
            body.clone(),
            "gemini-3-flash-preview(50000)",
            Protocol::Anthropic,
            "/v1/messages",
        );

        // clamp to max (32768)
        match result {
            InjectionResult::Injected(body) => {
                assert_eq!(body["model"], "gemini-3-flash-preview");
                assert_eq!(body["thinking"]["budget_tokens"], 32768);
            }
            _ => panic!("Expected Injected with budget_tokens: 32768 (clamped)"),
        }
    }
}
