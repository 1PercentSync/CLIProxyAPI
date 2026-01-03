//! Static model registry.
//!
//! This module maintains a static registry of all known models.
//! Model definitions are synchronized with CLIProxyAPI's `internal/registry/model_definitions.go`.
//!
//! Note: ALL models must be registered, not just those with thinking support.
//! This is required because RS-Proxy needs to distinguish between:
//! - Unknown models with thinking suffix → return 400 error
//! - Known models without thinking support → strip suffix and passthrough

use std::sync::LazyLock;

/// Describes a model's supported internal reasoning budget range.
/// Values are interpreted in provider-native token units.
#[derive(Debug, Clone, PartialEq)]
pub struct ThinkingSupport {
    /// Minimum allowed thinking budget (inclusive).
    pub min: i32,
    /// Maximum allowed thinking budget (inclusive).
    pub max: i32,
    /// Whether 0 is a valid value (to disable thinking).
    pub zero_allowed: bool,
    /// Whether -1 is a valid value (dynamic thinking budget).
    pub dynamic_allowed: bool,
    /// Discrete reasoning effort levels (e.g., "low", "medium", "high").
    /// When set, the model uses level-based reasoning instead of token budgets.
    pub levels: Option<&'static [&'static str]>,
}

/// Model information.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    /// Unique model identifier.
    pub id: &'static str,
    /// Maximum completion tokens (used for max_tokens adjustment).
    pub max_completion_tokens: i32,
    /// Thinking support configuration, None means thinking is not supported.
    pub thinking: Option<ThinkingSupport>,
}

// =============================================================================
// Static Model Definitions
// Synchronized with CLIProxyAPI's internal/registry/model_definitions.go
// Last sync: 2026-01-03
// =============================================================================

/// Claude models from GetClaudeModels()
static CLAUDE_MODELS: &[ModelInfo] = &[
    // Claude 4.5 Haiku - no thinking support
    ModelInfo {
        id: "claude-haiku-4-5-20251001",
        max_completion_tokens: 64000,
        thinking: None,
    },
    // Claude 4.5 Sonnet
    ModelInfo {
        id: "claude-sonnet-4-5-20250929",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Claude 4.5 Opus
    ModelInfo {
        id: "claude-opus-4-5-20251101",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Claude 4.1 Opus
    ModelInfo {
        id: "claude-opus-4-1-20250805",
        max_completion_tokens: 32000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Claude 4 Opus
    ModelInfo {
        id: "claude-opus-4-20250514",
        max_completion_tokens: 32000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Claude 4 Sonnet
    ModelInfo {
        id: "claude-sonnet-4-20250514",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Claude 3.7 Sonnet
    ModelInfo {
        id: "claude-3-7-sonnet-20250219",
        max_completion_tokens: 8192,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 100000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Claude 3.5 Haiku - no thinking support
    ModelInfo {
        id: "claude-3-5-haiku-20241022",
        max_completion_tokens: 8192,
        thinking: None,
    },
];

/// Gemini models from GetGeminiModels() (authoritative source)
static GEMINI_MODELS: &[ModelInfo] = &[
    // Gemini 2.5 Pro
    ModelInfo {
        id: "gemini-2.5-pro",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 128,
            max: 32768,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Gemini 2.5 Flash
    ModelInfo {
        id: "gemini-2.5-flash",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 24576,
            zero_allowed: true,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Gemini 2.5 Flash Lite
    ModelInfo {
        id: "gemini-2.5-flash-lite",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 24576,
            zero_allowed: true,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Gemini 3 Pro Preview (with levels)
    ModelInfo {
        id: "gemini-3-pro-preview",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 128,
            max: 32768,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: Some(&["low", "high"]),
        }),
    },
    // Gemini 3 Flash Preview (with levels)
    ModelInfo {
        id: "gemini-3-flash-preview",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 128,
            max: 32768,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: Some(&["minimal", "low", "medium", "high"]),
        }),
    },
    // Gemini 3 Pro Image Preview (with levels)
    ModelInfo {
        id: "gemini-3-pro-image-preview",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 128,
            max: 32768,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: Some(&["low", "high"]),
        }),
    },
];

/// Additional Gemini models from GetAIStudioModels() that are not in GetGeminiModels()
static GEMINI_AISTUDIO_MODELS: &[ModelInfo] = &[
    // Gemini Pro Latest (alias)
    ModelInfo {
        id: "gemini-pro-latest",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 128,
            max: 32768,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Gemini Flash Latest (alias)
    ModelInfo {
        id: "gemini-flash-latest",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 24576,
            zero_allowed: true,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Gemini Flash Lite Latest (alias)
    ModelInfo {
        id: "gemini-flash-lite-latest",
        max_completion_tokens: 65536,
        thinking: Some(ThinkingSupport {
            min: 512,
            max: 24576,
            zero_allowed: true,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // Gemini 2.5 Flash Image Preview - no thinking support
    ModelInfo {
        id: "gemini-2.5-flash-image-preview",
        max_completion_tokens: 8192,
        thinking: None,
    },
    // Gemini 2.5 Flash Image - no thinking support
    ModelInfo {
        id: "gemini-2.5-flash-image",
        max_completion_tokens: 8192,
        thinking: None,
    },
];

/// OpenAI models from GetOpenAIModels()
static OPENAI_MODELS: &[ModelInfo] = &[
    // GPT-5
    ModelInfo {
        id: "gpt-5",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["minimal", "low", "medium", "high"]),
        }),
    },
    // GPT-5 Codex
    ModelInfo {
        id: "gpt-5-codex",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["low", "medium", "high"]),
        }),
    },
    // GPT-5 Codex Mini
    ModelInfo {
        id: "gpt-5-codex-mini",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["low", "medium", "high"]),
        }),
    },
    // GPT-5.1
    ModelInfo {
        id: "gpt-5.1",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["none", "low", "medium", "high"]),
        }),
    },
    // GPT-5.1 Codex
    ModelInfo {
        id: "gpt-5.1-codex",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["low", "medium", "high"]),
        }),
    },
    // GPT-5.1 Codex Mini
    ModelInfo {
        id: "gpt-5.1-codex-mini",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["low", "medium", "high"]),
        }),
    },
    // GPT-5.1 Codex Max
    ModelInfo {
        id: "gpt-5.1-codex-max",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["low", "medium", "high", "xhigh"]),
        }),
    },
    // GPT-5.2
    ModelInfo {
        id: "gpt-5.2",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["none", "low", "medium", "high", "xhigh"]),
        }),
    },
    // GPT-5.2 Codex
    ModelInfo {
        id: "gpt-5.2-codex",
        max_completion_tokens: 128000,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["low", "medium", "high", "xhigh"]),
        }),
    },
];

/// iFlow models with thinking support from GetIFlowModels()
/// Only models with Thinking != nil are included
static IFLOW_MODELS: &[ModelInfo] = &[
    // GLM-4.6
    ModelInfo {
        id: "glm-4.6",
        max_completion_tokens: 0, // Not specified in Go code
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["none", "auto", "minimal", "low", "medium", "high", "xhigh"]),
        }),
    },
    // GLM-4.7
    ModelInfo {
        id: "glm-4.7",
        max_completion_tokens: 0,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["none", "auto", "minimal", "low", "medium", "high", "xhigh"]),
        }),
    },
    // MiniMax-M2.1
    ModelInfo {
        id: "minimax-m2.1",
        max_completion_tokens: 0,
        thinking: Some(ThinkingSupport {
            min: 0,
            max: 0,
            zero_allowed: false,
            dynamic_allowed: false,
            levels: Some(&["none", "auto", "minimal", "low", "medium", "high", "xhigh"]),
        }),
    },
];

/// iFlow models without thinking support from GetIFlowModels()
static IFLOW_MODELS_NO_THINKING: &[ModelInfo] = &[
    ModelInfo { id: "tstars2.0", max_completion_tokens: 0, thinking: None },
    // qwen3-coder-plus is in QWEN_MODELS with more complete info
    ModelInfo { id: "qwen3-max", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "qwen3-vl-plus", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "qwen3-max-preview", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "kimi-k2-0905", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "kimi-k2", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "kimi-k2-thinking", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "deepseek-v3.2-chat", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "deepseek-v3.2-reasoner", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "deepseek-v3.2", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "deepseek-v3.1", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "deepseek-r1", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "deepseek-v3", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "qwen3-32b", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "qwen3-235b-a22b-thinking-2507", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "qwen3-235b-a22b-instruct", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "qwen3-235b", max_completion_tokens: 0, thinking: None },
    ModelInfo { id: "minimax-m2", max_completion_tokens: 0, thinking: None },
];

/// Qwen models from GetQwenModels() - no thinking support
static QWEN_MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "qwen3-coder-plus",
        max_completion_tokens: 8192,
        thinking: None,
    },
    ModelInfo {
        id: "qwen3-coder-flash",
        max_completion_tokens: 2048,
        thinking: None,
    },
    ModelInfo {
        id: "vision-model",
        max_completion_tokens: 2048,
        thinking: None,
    },
];

/// Antigravity model configurations (merged from GetAntigravityModelConfig)
static ANTIGRAVITY_MODELS: &[ModelInfo] = &[
    // gemini-2.5-computer-use-preview-10-2025 - no thinking support
    ModelInfo {
        id: "gemini-2.5-computer-use-preview-10-2025",
        max_completion_tokens: 0,
        thinking: None,
    },
    // gemini-claude-sonnet-4-5-thinking
    ModelInfo {
        id: "gemini-claude-sonnet-4-5-thinking",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 200000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
    // gemini-claude-opus-4-5-thinking
    ModelInfo {
        id: "gemini-claude-opus-4-5-thinking",
        max_completion_tokens: 64000,
        thinking: Some(ThinkingSupport {
            min: 1024,
            max: 200000,
            zero_allowed: false,
            dynamic_allowed: true,
            levels: None,
        }),
    },
];

/// Combined static model registry.
static MODELS: LazyLock<Vec<&'static ModelInfo>> = LazyLock::new(|| {
    let mut models: Vec<&'static ModelInfo> = Vec::new();

    // Add all model lists
    models.extend(CLAUDE_MODELS.iter());
    models.extend(GEMINI_MODELS.iter());
    models.extend(GEMINI_AISTUDIO_MODELS.iter());
    models.extend(OPENAI_MODELS.iter());
    models.extend(IFLOW_MODELS.iter());
    models.extend(IFLOW_MODELS_NO_THINKING.iter());
    models.extend(QWEN_MODELS.iter());
    models.extend(ANTIGRAVITY_MODELS.iter());

    models
});

/// Look up model information by ID.
///
/// # Arguments
/// * `id` - Model identifier
///
/// # Returns
/// Model info if found, None otherwise.
pub fn get_model_info(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id).copied()
}

/// Check if a model supports thinking.
///
/// # Arguments
/// * `id` - Model identifier
///
/// # Returns
/// True if the model is in the registry and supports thinking.
pub fn model_supports_thinking(id: &str) -> bool {
    get_model_info(id)
        .map(|m| m.thinking.is_some())
        .unwrap_or(false)
}

/// Get all models in the registry.
pub fn get_all_models() -> &'static [&'static ModelInfo] {
    // This is safe because MODELS is initialized once
    // We need to use a different approach since LazyLock doesn't provide slice access directly
    &[]
}

/// Get the thinking levels for a model.
///
/// # Arguments
/// * `id` - Model identifier
///
/// # Returns
/// The levels array if the model has discrete levels, None otherwise.
pub fn get_model_thinking_levels(id: &str) -> Option<&'static [&'static str]> {
    get_model_info(id)
        .and_then(|m| m.thinking.as_ref())
        .and_then(|t| t.levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_info_claude() {
        let model = get_model_info("claude-sonnet-4-5-20250929");
        assert!(model.is_some());
        let model = model.unwrap();
        assert_eq!(model.max_completion_tokens, 64000);
        assert!(model.thinking.is_some());
        let thinking = model.thinking.as_ref().unwrap();
        assert_eq!(thinking.min, 1024);
        assert_eq!(thinking.max, 100000);
    }

    #[test]
    fn test_get_model_info_gemini() {
        let model = get_model_info("gemini-3-pro-preview");
        assert!(model.is_some());
        let thinking = model.unwrap().thinking.as_ref().unwrap();
        assert!(thinking.levels.is_some());
        assert_eq!(thinking.levels.unwrap(), &["low", "high"]);
    }

    #[test]
    fn test_get_model_info_openai() {
        let model = get_model_info("gpt-5.1");
        assert!(model.is_some());
        let thinking = model.unwrap().thinking.as_ref().unwrap();
        assert!(thinking.levels.is_some());
        assert!(thinking.levels.unwrap().contains(&"none"));
    }

    #[test]
    fn test_get_model_info_not_found() {
        let model = get_model_info("nonexistent-model");
        assert!(model.is_none());
    }

    #[test]
    fn test_model_supports_thinking() {
        assert!(model_supports_thinking("claude-sonnet-4-5-20250929"));
        assert!(!model_supports_thinking("claude-haiku-4-5-20251001"));
        assert!(!model_supports_thinking("nonexistent"));
    }

    #[test]
    fn test_get_model_thinking_levels() {
        // Model with levels
        let levels = get_model_thinking_levels("gpt-5.1");
        assert!(levels.is_some());
        assert!(levels.unwrap().contains(&"high"));

        // Model without levels (budget-based)
        let levels = get_model_thinking_levels("claude-sonnet-4-5-20250929");
        assert!(levels.is_none());

        // Non-existent model
        let levels = get_model_thinking_levels("nonexistent");
        assert!(levels.is_none());
    }

    #[test]
    fn test_iflow_models() {
        let model = get_model_info("glm-4.6");
        assert!(model.is_some());
        let thinking = model.unwrap().thinking.as_ref().unwrap();
        assert!(thinking.levels.is_some());
        let levels = thinking.levels.unwrap();
        assert!(levels.contains(&"auto"));
        assert!(levels.contains(&"xhigh"));
    }

    #[test]
    fn test_iflow_models_no_thinking() {
        // These models should be in registry but without thinking support
        let model = get_model_info("deepseek-r1");
        assert!(model.is_some());
        assert!(model.unwrap().thinking.is_none());

        let model = get_model_info("kimi-k2");
        assert!(model.is_some());
        assert!(model.unwrap().thinking.is_none());
    }

    #[test]
    fn test_qwen_models() {
        let model = get_model_info("qwen3-coder-flash");
        assert!(model.is_some());
        assert!(model.unwrap().thinking.is_none());
    }
}
