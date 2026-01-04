//! Thinking configuration module.
//!
//! This module handles parsing model name suffixes, mapping effort levels
//! to budgets, and injecting thinking configuration into requests.
//!
//! # Design: Intent-based Processing
//!
//! User input is first classified into one of three intents:
//! - `Disabled`: User wants to disable thinking (`(none)` or `(0)`)
//! - `Dynamic`: User wants dynamic/auto thinking (`(auto)` or `(-1)`)
//! - `Fixed`: User specifies a concrete level or budget
//!
//! This classification happens before protocol-specific adaptation,
//! allowing clean separation of user intent from protocol requirements.

pub mod injector;
pub mod models;
pub mod parser;

pub use injector::{inject_thinking_config, InjectionError, InjectionResult};
pub use models::{budget_to_effort, clamp_budget, clamp_effort_to_levels, level_to_budget};
pub use parser::{parse_model_suffix, ParsedModel, ThinkingValue};
// ThinkingIntent and FixedThinking are defined in this file and re-exported

/// User's thinking intent, classified from parsed suffix.
///
/// This intermediate type separates user intent from protocol-specific output format.
/// It allows handling of special values (`none`, `auto`, `0`, `-1`) at the intent level,
/// before any protocol-specific conversion.
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingIntent {
    /// Disable thinking: user requested `(none)` or `(0)`.
    ///
    /// Protocol adaptation:
    /// - Anthropic: `thinking: { type: "disabled" }`
    /// - OpenAI: `reasoning_effort: "none"`
    /// - Gemini (with levels): `thinkingBudget: 0`
    /// - Gemini 2.5 (native, no levels): clamp to min
    /// - Gemini (cross-protocol): `thinkingBudget: 0`
    Disabled,

    /// Dynamic thinking: user requested `(auto)` or `(-1)`.
    ///
    /// Let the API decide the thinking budget dynamically.
    ///
    /// Protocol adaptation:
    /// - Anthropic: use `auto_budget` or `(min+max)/2` (API doesn't support -1)
    /// - OpenAI: `reasoning_effort: "medium"` (API doesn't support auto)
    /// - Gemini: `thinkingBudget: -1` (API supports dynamic)
    Dynamic,

    /// Fixed thinking: user specified a concrete level or budget.
    ///
    /// The value will be clamped to model's supported range and
    /// converted to the appropriate protocol format.
    Fixed(FixedThinking),
}

/// Fixed thinking value, either a level string or numeric budget.
#[derive(Debug, Clone, PartialEq)]
pub enum FixedThinking {
    /// User specified a level string: `(high)`, `(low)`, `(medium)`, etc.
    Level(String),

    /// User specified a numeric budget: `(8192)`, `(16384)`, etc.
    Budget(i32),
}

/// Thinking configuration type.
///
/// Represents a parsed and converted thinking configuration ready for injection.
/// The specific type depends on the target protocol:
/// - OpenAI protocol: uses `Effort` (level string) or `Disabled`
/// - Anthropic protocol: uses `Budget` (numeric value) or `Disabled`
/// - Gemini protocol: depends on model version (2.5 uses Budget, 3 uses Effort)
///
/// # Design Decision: Disabled Thinking
///
/// When `level = "none"` or `budget = 0`, thinking is disabled:
/// - Anthropic protocol: injects `thinking: { type: "disabled" }`
/// - OpenAI protocol: injects `reasoning_effort: "none"`
/// - Gemini protocol: does not inject thinking config (omission = disabled)
///
/// This differs from CLIProxyAPI which simply omits the thinking config entirely.
/// We explicitly inject disabled state for Anthropic to be more explicit about user intent.
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingConfig {
    /// Numeric budget (tokens), used for Anthropic and Gemini 2.5.
    Budget(i32),
    /// Effort level string, used for OpenAI and Gemini 3.
    Effort(String),
    /// Thinking disabled (level="none" or budget=0).
    /// Protocol handlers should inject appropriate disabled state.
    Disabled,
}
