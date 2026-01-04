//! Gemini protocol thinking injection.
//!
//! This module handles injecting thinking configuration into Gemini API requests.
//! It supports both Gemini 2.5 (budget-based) and Gemini 3 (level-based) formats.

use crate::thinking::ThinkingConfig;

/// Inject Gemini thinking configuration into request body.
///
/// This function determines the injection format based on the ThinkingConfig type:
/// - `Budget` → Gemini 2.5 format (thinkingBudget + include_thoughts snake_case)
/// - `Effort` → Gemini 3 format (thinkingLevel + includeThoughts camelCase)
/// - `Disabled` → Should never be received (injector converts to Budget for Gemini)
///
/// The injection follows a "clean then set" pattern for thinking values,
/// but respects user's include_thoughts/includeThoughts settings:
/// 1. Remove existing thinkingBudget/thinkingLevel fields
/// 2. Set the new thinking field based on config type
/// 3. Only set include_thoughts/includeThoughts if user hasn't set either
///
/// Note: The injector decides which type to pass based on the model's `levels` field,
/// so this module doesn't need to check the model version.
///
/// For Gemini protocol, Disabled intent is converted by injector:
/// - Gemini 3 (has levels) → Budget(0)
/// - Gemini 2.5 (native) → Budget(min)
/// - Cross-protocol → Budget(0)
///
/// # Arguments
/// * `body` - Request body JSON
/// * `base_model` - Base model name (suffix stripped)
/// * `thinking_config` - Processed thinking config
pub fn inject_gemini(
    mut body: serde_json::Value,
    base_model: &str,
    thinking_config: ThinkingConfig,
) -> serde_json::Value {
    // Update model name (strip suffix)
    body["model"] = serde_json::Value::String(base_model.to_string());

    // Note: For Gemini protocol, injector converts Disabled intent to Budget(0) or Budget(min),
    // so we should never receive ThinkingConfig::Disabled here.
    // If we do, it's a programming error, but we handle it gracefully by doing nothing.
    if matches!(thinking_config, ThinkingConfig::Disabled) {
        // This should not happen for Gemini protocol
        // Injector converts Disabled → Budget(0) or Budget(min)
        return body;
    }

    // Ensure generationConfig.thinkingConfig exists
    if body.get("generationConfig").is_none() {
        body["generationConfig"] = serde_json::json!({});
    }
    if body["generationConfig"].get("thinkingConfig").is_none() {
        body["generationConfig"]["thinkingConfig"] = serde_json::json!({});
    }

    let thinking_config_obj = &mut body["generationConfig"]["thinkingConfig"];

    // Check if user has set include_thoughts or includeThoughts
    let user_set_include_thoughts = thinking_config_obj.get("include_thoughts").is_some()
        || thinking_config_obj.get("includeThoughts").is_some();

    // Step 1: Clean existing thinking value fields (but preserve include_thoughts/includeThoughts)
    if let Some(obj) = thinking_config_obj.as_object_mut() {
        obj.remove("thinkingBudget");
        obj.remove("thinkingLevel");
    }

    // Step 2: Set new fields based on config type
    match thinking_config {
        ThinkingConfig::Disabled => unreachable!("Handled above"),
        ThinkingConfig::Budget(budget) => {
            // Gemini 2.5 format: uses numeric budget + snake_case include_thoughts
            // Note: budget=0 means disable thinking (for Gemini 3 with levels)
            // Note: budget=min means disable thinking (for Gemini 2.5 native, clamped)
            thinking_config_obj["thinkingBudget"] = serde_json::Value::Number(budget.into());

            // Only set include_thoughts if user hasn't set either variant
            if !user_set_include_thoughts {
                thinking_config_obj["include_thoughts"] = serde_json::Value::Bool(true);
            }
        }
        ThinkingConfig::Effort(level) => {
            // Gemini 3 format: uses discrete level + camelCase includeThoughts
            thinking_config_obj["thinkingLevel"] = serde_json::Value::String(level);

            // Only set includeThoughts if user hasn't set either variant
            if !user_set_include_thoughts {
                thinking_config_obj["includeThoughts"] = serde_json::Value::Bool(true);
            }
        }
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_inject_gemini_25_budget() {
        let body = json!({
            "model": "gemini-2.5-pro(16384)",
            "contents": [{"parts": [{"text": "Hello"}]}]
        });

        let result = inject_gemini(body, "gemini-2.5-pro", ThinkingConfig::Budget(16384));

        assert_eq!(result["model"], "gemini-2.5-pro");
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            16384
        );
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["include_thoughts"],
            true
        );
        // Should not have Gemini 3 fields
        assert!(result["generationConfig"]["thinkingConfig"]
            .get("thinkingLevel")
            .is_none());
    }

    #[test]
    fn test_inject_gemini_3_level() {
        let body = json!({
            "model": "gemini-3-pro-preview(high)",
            "contents": []
        });

        let result = inject_gemini(
            body,
            "gemini-3-pro-preview",
            ThinkingConfig::Effort("high".to_string()),
        );

        assert_eq!(result["model"], "gemini-3-pro-preview");
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "high"
        );
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["includeThoughts"],
            true
        );
        // Should not have Gemini 2.5 fields
        assert!(result["generationConfig"]["thinkingConfig"]
            .get("thinkingBudget")
            .is_none());
        assert!(result["generationConfig"]["thinkingConfig"]
            .get("include_thoughts")
            .is_none());
    }

    #[test]
    fn test_preserve_existing_include_thoughts_25() {
        let body = json!({
            "model": "gemini-2.5-pro(8192)",
            "generationConfig": {
                "thinkingConfig": {
                    "include_thoughts": false
                }
            }
        });

        let result = inject_gemini(body, "gemini-2.5-pro", ThinkingConfig::Budget(8192));

        // Should preserve user's setting
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["include_thoughts"],
            false
        );
    }

    #[test]
    fn test_preserve_existing_include_thoughts_3() {
        let body = json!({
            "model": "gemini-3-pro-preview(high)",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": false
                }
            }
        });

        let result = inject_gemini(
            body,
            "gemini-3-pro-preview",
            ThinkingConfig::Effort("high".to_string()),
        );

        // Should preserve user's setting
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["includeThoughts"],
            false
        );
    }

    #[test]
    fn test_preserve_snake_case_for_gemini_3() {
        // User set snake_case include_thoughts, but we're injecting Gemini 3 format
        // Should preserve user's snake_case setting (no version conversion)
        let body = json!({
            "model": "gemini-3-pro-preview(high)",
            "generationConfig": {
                "thinkingConfig": {
                    "include_thoughts": false
                }
            }
        });

        let result = inject_gemini(
            body,
            "gemini-3-pro-preview",
            ThinkingConfig::Effort("high".to_string()),
        );

        // Should preserve user's snake_case setting
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["include_thoughts"],
            false
        );
        // Should NOT add camelCase version
        assert!(result["generationConfig"]["thinkingConfig"]
            .get("includeThoughts")
            .is_none());
    }

    #[test]
    fn test_preserve_camel_case_for_gemini_25() {
        // User set camelCase includeThoughts, but we're injecting Gemini 2.5 format
        // Should preserve user's camelCase setting (no version conversion)
        let body = json!({
            "model": "gemini-2.5-pro(8192)",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": false
                }
            }
        });

        let result = inject_gemini(body, "gemini-2.5-pro", ThinkingConfig::Budget(8192));

        // Should preserve user's camelCase setting
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["includeThoughts"],
            false
        );
        // Should NOT add snake_case version
        assert!(result["generationConfig"]["thinkingConfig"]
            .get("include_thoughts")
            .is_none());
    }

    #[test]
    fn test_cleanup_thinking_budget_for_gemini_3() {
        let body = json!({
            "model": "gemini-3-pro-preview(high)",
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingBudget": 8192,
                    "include_thoughts": true
                }
            }
        });

        let result = inject_gemini(
            body,
            "gemini-3-pro-preview",
            ThinkingConfig::Effort("high".to_string()),
        );

        // Should remove thinkingBudget
        assert!(result["generationConfig"]["thinkingConfig"]
            .get("thinkingBudget")
            .is_none());
        // Should preserve user's include_thoughts (no version conversion)
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["include_thoughts"],
            true
        );
        // Should have Gemini 3 thinkingLevel
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "high"
        );
        // Should NOT add includeThoughts since user set include_thoughts
        assert!(result["generationConfig"]["thinkingConfig"]
            .get("includeThoughts")
            .is_none());
    }

    #[test]
    fn test_inject_disabled_fallback() {
        // Note: This should never happen in practice.
        // Injector converts Disabled → Budget(0) or Budget(min) for Gemini protocol.
        // This test verifies the graceful fallback behavior.
        let body = json!({
            "model": "gemini-2.5-pro(none)",
            "contents": []
        });

        let result = inject_gemini(body, "gemini-2.5-pro", ThinkingConfig::Disabled);

        assert_eq!(result["model"], "gemini-2.5-pro");
        // Should not have any thinking config (graceful fallback)
        assert!(result.get("generationConfig").is_none());
    }

    #[test]
    fn test_inject_budget_zero_for_disable() {
        // This is the actual way Gemini disables thinking: Budget(0)
        let body = json!({
            "model": "gemini-3-pro-preview(none)",
            "contents": []
        });

        let result = inject_gemini(body, "gemini-3-pro-preview", ThinkingConfig::Budget(0));

        assert_eq!(result["model"], "gemini-3-pro-preview");
        // Should have thinkingBudget: 0
        assert_eq!(
            result["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
    }
}
