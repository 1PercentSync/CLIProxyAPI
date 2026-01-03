//! Model name suffix parser.
//!
//! This module parses thinking configuration from model name suffixes,
//! consistent with CLIProxyAPI's `NormalizeThinkingModel()` logic.

use super::{FixedThinking, ThinkingIntent};

/// Thinking configuration value type.
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingValue {
    /// No suffix or empty parentheses `()`.
    None,
    /// Numeric budget (e.g., 16384, -1).
    Budget(i32),
    /// Level string (e.g., "high", "auto", "none").
    Level(String),
}

impl ThinkingValue {
    /// Convert parsed value to user intent.
    ///
    /// This classifies the raw parsed value into one of three intents:
    /// - `Disabled`: `(none)` or `(0)` → user wants to disable thinking
    /// - `Dynamic`: `(auto)` or `(-1)` → user wants dynamic/auto thinking
    /// - `Fixed`: other levels or budgets → user specified a concrete value
    ///
    /// Returns `None` for `ThinkingValue::None` (no suffix).
    pub fn to_intent(&self) -> Option<ThinkingIntent> {
        match self {
            ThinkingValue::None => None,
            ThinkingValue::Budget(0) => Some(ThinkingIntent::Disabled),
            ThinkingValue::Budget(-1) => Some(ThinkingIntent::Dynamic),
            ThinkingValue::Budget(b) => Some(ThinkingIntent::Fixed(FixedThinking::Budget(*b))),
            ThinkingValue::Level(level) => {
                let level_lower = level.to_lowercase();
                match level_lower.as_str() {
                    "none" => Some(ThinkingIntent::Disabled),
                    "auto" => Some(ThinkingIntent::Dynamic),
                    _ => Some(ThinkingIntent::Fixed(FixedThinking::Level(level_lower))),
                }
            }
        }
    }
}

/// Parsed model information.
#[derive(Debug, Clone)]
pub struct ParsedModel {
    /// Base model name (suffix stripped).
    pub base_name: String,
    /// Thinking configuration value.
    pub thinking: ThinkingValue,
}

/// Parse model name suffix.
///
/// Extracts base model name and thinking configuration from a model name.
///
/// # Examples
/// - `claude-sonnet-4(16384)` → base_name: "claude-sonnet-4", thinking: Budget(16384)
/// - `gpt-5.1(high)` → base_name: "gpt-5.1", thinking: Level("high")
/// - `claude-sonnet-4()` → base_name: "claude-sonnet-4", thinking: None
/// - `claude-sonnet-4` → base_name: "claude-sonnet-4", thinking: None
/// - `model(high` → base_name: "model(high", thinking: None (incomplete parentheses, passthrough)
///
/// # Design Decision (differs from CLIProxyAPI)
/// RS-Proxy strips empty parentheses `model()` to use `model` as the base name.
/// CLIProxyAPI returns the original model name (with parentheses) for empty suffix.
pub fn parse_model_suffix(model: &str) -> ParsedModel {
    // Find the last '(' and ')'
    let open_paren = match model.rfind('(') {
        Some(idx) => idx,
        None => {
            // No parentheses, return as-is
            return ParsedModel {
                base_name: model.to_string(),
                thinking: ThinkingValue::None,
            };
        }
    };

    let close_paren = match model.rfind(')') {
        Some(idx) => idx,
        None => {
            // Only '(' without ')', incomplete parentheses, passthrough
            return ParsedModel {
                base_name: model.to_string(),
                thinking: ThinkingValue::None,
            };
        }
    };

    // Check parentheses order: ')' must be after '(' and at the end
    if close_paren <= open_paren || close_paren != model.len() - 1 {
        // Wrong order or ')' not at the end, passthrough
        return ParsedModel {
            base_name: model.to_string(),
            thinking: ThinkingValue::None,
        };
    }

    // Extract base name and suffix content
    let base_name = model[..open_paren].to_string();
    let suffix = &model[open_paren + 1..close_paren];

    // Empty suffix
    if suffix.is_empty() {
        return ParsedModel {
            base_name,
            thinking: ThinkingValue::None,
        };
    }

    // Try parsing as numeric
    if let Ok(budget) = suffix.parse::<i32>() {
        return ParsedModel {
            base_name,
            thinking: ThinkingValue::Budget(budget),
        };
    }

    // Level string
    ParsedModel {
        base_name,
        thinking: ThinkingValue::Level(suffix.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_numeric_budget() {
        let result = parse_model_suffix("claude-sonnet-4(16384)");
        assert_eq!(result.base_name, "claude-sonnet-4");
        assert_eq!(result.thinking, ThinkingValue::Budget(16384));
    }

    #[test]
    fn test_parse_negative_budget() {
        let result = parse_model_suffix("model(-1)");
        assert_eq!(result.base_name, "model");
        assert_eq!(result.thinking, ThinkingValue::Budget(-1));
    }

    #[test]
    fn test_parse_zero_budget() {
        let result = parse_model_suffix("model(0)");
        assert_eq!(result.base_name, "model");
        assert_eq!(result.thinking, ThinkingValue::Budget(0));
    }

    #[test]
    fn test_parse_level_string() {
        let result = parse_model_suffix("gpt-5.1(high)");
        assert_eq!(result.base_name, "gpt-5.1");
        assert_eq!(result.thinking, ThinkingValue::Level("high".to_string()));
    }

    #[test]
    fn test_parse_level_case_preserved() {
        let result = parse_model_suffix("gpt-5.1(HIGH)");
        assert_eq!(result.base_name, "gpt-5.1");
        assert_eq!(result.thinking, ThinkingValue::Level("HIGH".to_string()));
    }

    #[test]
    fn test_parse_no_suffix() {
        let result = parse_model_suffix("claude-sonnet-4");
        assert_eq!(result.base_name, "claude-sonnet-4");
        assert_eq!(result.thinking, ThinkingValue::None);
    }

    #[test]
    fn test_parse_empty_parentheses() {
        let result = parse_model_suffix("model-name()");
        assert_eq!(result.base_name, "model-name");
        assert_eq!(result.thinking, ThinkingValue::None);
    }

    #[test]
    fn test_parse_incomplete_open() {
        let result = parse_model_suffix("model(high");
        assert_eq!(result.base_name, "model(high");
        assert_eq!(result.thinking, ThinkingValue::None);
    }

    #[test]
    fn test_parse_incomplete_close() {
        let result = parse_model_suffix("model)high");
        assert_eq!(result.base_name, "model)high");
        assert_eq!(result.thinking, ThinkingValue::None);
    }

    #[test]
    fn test_parse_wrong_order() {
        let result = parse_model_suffix("model)high(");
        assert_eq!(result.base_name, "model)high(");
        assert_eq!(result.thinking, ThinkingValue::None);
    }

    #[test]
    fn test_parse_close_not_at_end() {
        let result = parse_model_suffix("model(high)suffix");
        assert_eq!(result.base_name, "model(high)suffix");
        assert_eq!(result.thinking, ThinkingValue::None);
    }

    #[test]
    fn test_parse_nested_parentheses() {
        // Uses the last ( and )
        let result = parse_model_suffix("model(a)(high)");
        assert_eq!(result.base_name, "model(a)");
        assert_eq!(result.thinking, ThinkingValue::Level("high".to_string()));
    }

    #[test]
    fn test_parse_special_levels() {
        let result = parse_model_suffix("model(none)");
        assert_eq!(result.thinking, ThinkingValue::Level("none".to_string()));

        let result = parse_model_suffix("model(auto)");
        assert_eq!(result.thinking, ThinkingValue::Level("auto".to_string()));

        let result = parse_model_suffix("model(xhigh)");
        assert_eq!(result.thinking, ThinkingValue::Level("xhigh".to_string()));
    }

    // ===== to_intent() Tests =====

    #[test]
    fn test_to_intent_none_value() {
        assert_eq!(ThinkingValue::None.to_intent(), None);
    }

    #[test]
    fn test_to_intent_disabled() {
        // Budget 0 → Disabled
        assert_eq!(
            ThinkingValue::Budget(0).to_intent(),
            Some(ThinkingIntent::Disabled)
        );

        // Level "none" → Disabled
        assert_eq!(
            ThinkingValue::Level("none".to_string()).to_intent(),
            Some(ThinkingIntent::Disabled)
        );

        // Case insensitive
        assert_eq!(
            ThinkingValue::Level("NONE".to_string()).to_intent(),
            Some(ThinkingIntent::Disabled)
        );
    }

    #[test]
    fn test_to_intent_dynamic() {
        // Budget -1 → Dynamic
        assert_eq!(
            ThinkingValue::Budget(-1).to_intent(),
            Some(ThinkingIntent::Dynamic)
        );

        // Level "auto" → Dynamic
        assert_eq!(
            ThinkingValue::Level("auto".to_string()).to_intent(),
            Some(ThinkingIntent::Dynamic)
        );

        // Case insensitive
        assert_eq!(
            ThinkingValue::Level("AUTO".to_string()).to_intent(),
            Some(ThinkingIntent::Dynamic)
        );
    }

    #[test]
    fn test_to_intent_fixed_budget() {
        assert_eq!(
            ThinkingValue::Budget(8192).to_intent(),
            Some(ThinkingIntent::Fixed(FixedThinking::Budget(8192)))
        );

        assert_eq!(
            ThinkingValue::Budget(16384).to_intent(),
            Some(ThinkingIntent::Fixed(FixedThinking::Budget(16384)))
        );

        // Negative budgets (other than -1) are still Fixed
        assert_eq!(
            ThinkingValue::Budget(-100).to_intent(),
            Some(ThinkingIntent::Fixed(FixedThinking::Budget(-100)))
        );
    }

    #[test]
    fn test_to_intent_fixed_level() {
        assert_eq!(
            ThinkingValue::Level("high".to_string()).to_intent(),
            Some(ThinkingIntent::Fixed(FixedThinking::Level("high".to_string())))
        );

        assert_eq!(
            ThinkingValue::Level("low".to_string()).to_intent(),
            Some(ThinkingIntent::Fixed(FixedThinking::Level("low".to_string())))
        );

        // Levels are lowercased
        assert_eq!(
            ThinkingValue::Level("HIGH".to_string()).to_intent(),
            Some(ThinkingIntent::Fixed(FixedThinking::Level("high".to_string())))
        );
    }
}
