//! Level and budget mapping functions.
//!
//! This module provides functions to convert between effort levels and budgets,
//! and to clamp values to model-supported ranges.

/// Level to budget mapping (forward, generic).
///
/// Maps effort level strings to token budgets.
/// Returns None for unknown levels.
pub fn level_to_budget(level: &str) -> Option<i32> {
    match level.to_lowercase().as_str() {
        "none" => Some(0),
        "auto" => Some(-1),
        "minimal" => Some(512),
        "low" => Some(1024),
        "medium" => Some(8192),
        "high" => Some(24576),
        "xhigh" => Some(32768),
        _ => None,
    }
}

/// Budget to effort level (reverse mapping, needed for OpenAI protocol).
///
/// This mapping is symmetric with `level_to_budget`:
/// - none: 0
/// - auto: -1
/// - minimal: 1 ~ 512
/// - low: 513 ~ 1024
/// - medium: 1025 ~ 8192
/// - high: 8193 ~ 24576
/// - xhigh: > 24576
///
/// For models with discrete levels, the result should be further clamped
/// using `clamp_effort_to_levels`.
///
/// # Arguments
/// * `budget` - Token budget value
///
/// # Returns
/// Effort level string (static, from generic mapping).
pub fn budget_to_effort(budget: i32) -> &'static str {
    match budget {
        0 => "none",
        -1 => "auto",
        1..=512 => "minimal",
        513..=1024 => "low",
        1025..=8192 => "medium",
        8193..=24576 => "high",
        _ if budget > 24576 => "xhigh",
        _ => "medium", // Negative (except -1) fallback
    }
}

/// Clamp budget to model-supported range.
///
/// Handles special values 0 and -1 based on model configuration.
///
/// # Arguments
/// * `budget` - Input budget value
/// * `min` - Model's minimum budget
/// * `max` - Model's maximum budget
/// * `zero_allowed` - Whether 0 is allowed (to disable thinking)
/// * `dynamic_allowed` - Whether -1 is allowed (dynamic thinking budget)
/// * `auto_budget` - Custom budget for -1 when dynamic_allowed=false (falls back to (min+max)/2 if None)
pub fn clamp_budget(
    budget: i32,
    min: i32,
    max: i32,
    zero_allowed: bool,
    dynamic_allowed: bool,
    auto_budget: Option<i32>,
) -> i32 {
    match budget {
        0 if !zero_allowed => min,
        -1 if !dynamic_allowed => auto_budget.unwrap_or((min + max) / 2),
        _ if budget > 0 && budget < min => min,
        _ if budget > max => max,
        _ => budget,
    }
}

/// Level order for clamping.
const LEVEL_ORDER: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh"];

/// Clamp effort level to model-supported discrete levels.
///
/// Levels not in the supported list are clamped up to the nearest supported level.
/// If no higher level exists, returns the highest available level.
///
/// Note: "auto" is handled at the intent level and should not reach this function.
/// If it does, it will be treated as "medium" (default fallback position).
///
/// # Arguments
/// * `effort` - Input effort level (should be a concrete level, not "auto")
/// * `levels` - Model's supported discrete levels
///
/// # Returns
/// Clamped effort level from the model's supported list.
pub fn clamp_effort_to_levels<'a>(effort: &str, levels: &'a [&'a str]) -> &'a str {
    // If level is in supported list, return it directly
    if levels.contains(&effort) {
        return levels.iter().find(|&&l| l == effort).unwrap();
    }

    // Find input level's position in standard order
    // "auto" is not in LEVEL_ORDER, so it will get default position 3 (medium)
    let effort_idx = LEVEL_ORDER.iter().position(|&l| l == effort).unwrap_or(3); // default to medium

    // Clamp up: find first supported level >= current level
    for &level in LEVEL_ORDER[effort_idx..].iter() {
        if levels.contains(&level) {
            return levels.iter().find(|&&l| l == level).unwrap();
        }
    }

    // If no higher level found, return highest supported level
    levels.last().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_to_budget() {
        assert_eq!(level_to_budget("none"), Some(0));
        assert_eq!(level_to_budget("auto"), Some(-1));
        assert_eq!(level_to_budget("minimal"), Some(512));
        assert_eq!(level_to_budget("low"), Some(1024));
        assert_eq!(level_to_budget("medium"), Some(8192));
        assert_eq!(level_to_budget("high"), Some(24576));
        assert_eq!(level_to_budget("xhigh"), Some(32768));
        assert_eq!(level_to_budget("unknown"), None);
    }

    #[test]
    fn test_level_to_budget_case_insensitive() {
        assert_eq!(level_to_budget("HIGH"), Some(24576));
        assert_eq!(level_to_budget("Medium"), Some(8192));
    }

    #[test]
    fn test_budget_to_effort_symmetric() {
        // Symmetric mapping with level_to_budget
        assert_eq!(budget_to_effort(0), "none");
        assert_eq!(budget_to_effort(-1), "auto");
        assert_eq!(budget_to_effort(1), "minimal");
        assert_eq!(budget_to_effort(512), "minimal");
        assert_eq!(budget_to_effort(513), "low");
        assert_eq!(budget_to_effort(1024), "low");
        assert_eq!(budget_to_effort(1025), "medium");
        assert_eq!(budget_to_effort(8192), "medium");
        assert_eq!(budget_to_effort(8193), "high");
        assert_eq!(budget_to_effort(24576), "high");
        assert_eq!(budget_to_effort(24577), "xhigh");
        assert_eq!(budget_to_effort(32768), "xhigh");
        assert_eq!(budget_to_effort(100000), "xhigh");
    }

    #[test]
    fn test_budget_to_effort_roundtrip() {
        // Verify roundtrip: level -> budget -> level
        for level in &["none", "auto", "minimal", "low", "medium", "high", "xhigh"] {
            let budget = level_to_budget(level).unwrap();
            let back = budget_to_effort(budget);
            assert_eq!(*level, back, "Roundtrip failed for {}", level);
        }
    }

    #[test]
    fn test_budget_to_effort_negative_fallback() {
        // Negative values (except -1) fall back to medium
        assert_eq!(budget_to_effort(-2), "medium");
        assert_eq!(budget_to_effort(-100), "medium");
    }

    #[test]
    fn test_clamp_budget_normal() {
        // Normal range
        assert_eq!(clamp_budget(8192, 1024, 100000, false, true, None), 8192);
        // Below min
        assert_eq!(clamp_budget(500, 1024, 100000, false, true, None), 1024);
        // Above max
        assert_eq!(clamp_budget(200000, 1024, 100000, false, true, None), 100000);
    }

    #[test]
    fn test_clamp_budget_zero() {
        // Zero allowed
        assert_eq!(clamp_budget(0, 1024, 100000, true, true, None), 0);
        // Zero not allowed
        assert_eq!(clamp_budget(0, 1024, 100000, false, true, None), 1024);
    }

    #[test]
    fn test_clamp_budget_dynamic() {
        // Dynamic allowed
        assert_eq!(clamp_budget(-1, 1024, 100000, false, true, None), -1);
        // Dynamic not allowed, no auto_budget (clamp to midpoint)
        assert_eq!(clamp_budget(-1, 1024, 100000, false, false, None), 50512); // (1024 + 100000) / 2
        // Dynamic not allowed, with auto_budget
        assert_eq!(clamp_budget(-1, 1024, 100000, false, false, Some(16384)), 16384);
    }

    #[test]
    fn test_clamp_effort_in_list() {
        let levels: &[&str] = &["low", "medium", "high"];
        assert_eq!(clamp_effort_to_levels("low", levels), "low");
        assert_eq!(clamp_effort_to_levels("medium", levels), "medium");
        assert_eq!(clamp_effort_to_levels("high", levels), "high");
    }

    #[test]
    fn test_clamp_effort_not_in_list() {
        let levels: &[&str] = &["low", "high"];
        // "medium" not in list, clamp up to "high"
        assert_eq!(clamp_effort_to_levels("medium", levels), "high");
        // "minimal" not in list, clamp up to "low"
        assert_eq!(clamp_effort_to_levels("minimal", levels), "low");
    }

    #[test]
    fn test_clamp_effort_xhigh_fallback() {
        let levels: &[&str] = &["low", "medium"];
        // "xhigh" not in list and no higher level, use highest available
        assert_eq!(clamp_effort_to_levels("xhigh", levels), "medium");
        assert_eq!(clamp_effort_to_levels("high", levels), "medium");
    }

    #[test]
    fn test_clamp_effort_auto() {
        // "auto" is handled at intent level and shouldn't reach here
        // If it does reach here, it gets default position (medium)
        // since "auto" is not in LEVEL_ORDER
        let levels: &[&str] = &["low", "medium", "high"];
        assert_eq!(clamp_effort_to_levels("auto", levels), "medium");
    }

    #[test]
    fn test_clamp_effort_none() {
        let levels_with_none: &[&str] = &["none", "low", "medium", "high"];
        assert_eq!(clamp_effort_to_levels("none", levels_with_none), "none");

        let levels_without_none: &[&str] = &["low", "medium", "high"];
        // "none" not in list, clamp up to "low"
        assert_eq!(clamp_effort_to_levels("none", levels_without_none), "low");
    }
}
