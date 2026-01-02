## ADDED Requirements

### Requirement: Gemini 协议思考注入

The system SHALL为 Gemini 协议注入思考配置，与 CLIProxyAPI 保持一致。

**文件：** `src/protocol/gemini.rs`

#### Scenario: 带思考预算的 Gemini 2.5
- **当** 模型为 Gemini 2.5 变体（如 `gemini-2.5-pro`、`gemini-2.5-flash`）
- **且** 带有思考后缀
- **则** The system SHALL将 `generationConfig.thinkingConfig.thinkingBudget` 设为数值
- **且** 若未显式设置，将 `generationConfig.thinkingConfig.include_thoughts` 设为 `true`

#### Scenario: 带思考等级的 Gemini 3
- **当** 模型为 Gemini 3 变体（如 `gemini-3-pro-preview`、`gemini-3-flash-preview`）
- **且** 带有思考后缀
- **则** The system SHALL将预算转换为等级字符串并设置 `generationConfig.thinkingConfig.thinkingLevel`
- **且** 若未显式设置，将 `generationConfig.thinkingConfig.includeThoughts` 设为 `true`
- **且** 若存在 `thinkingBudget` 字段则移除（Gemini 3 使用等级而非预算）

#### Scenario: Gemini 3 预算到等级转换
- **当** 模型为 Gemini 3 且带有数值预算时
- **则** The system SHALL使用以下规则转换：
  - 对于 Gemini 3 Pro：仅支持 `"low"`、`"high"`
    - budget <= 1024 → `"low"`
    - budget > 1024 → `"high"`
  - 对于 Gemini 3 Flash：支持 `"minimal"`、`"low"`、`"medium"`、`"high"`
    - budget <= 512 → `"minimal"`
    - budget <= 1024 → `"low"`
    - budget <= 8192 → `"medium"`
    - budget > 8192 → `"high"`
  - budget == -1（auto）→ `"high"`

#### Scenario: 带 auto 等级的 Gemini
- **当** 模型为 Gemini 且带有后缀 `(auto)` 时
- **则** 对于 Gemini 2.5：将 `thinkingBudget` 设为 `-1`
- **且** 对于 Gemini 3：将 `thinkingLevel` 设为 `"high"`

#### Scenario: 带默认思考的 Gemini 模型
- **当** 模型默认启用思考（如 `gemini-3-pro-preview`）
- **且** 未提供后缀
- **则** The system SHALL不禁用思考
- **且** 让模型使用其默认行为

#### Scenario: 未知模型带思考后缀
- **当** 模型带有思考后缀（如 `(high)`、`(16384)`）
- **且** 模型不存在于注册表中
- **则** The system SHALL返回 HTTP 400 错误，说明模型未知

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> RS-Proxy 要求模型必须在注册表中才能应用思考配置。
> 详见 thinking-mapping/spec.md 了解此设计决策的完整说明。

### Requirement: 自动设置 include_thoughts

The system SHALL在配置思考时自动设置 include_thoughts。

#### Scenario: Gemini 2.5 自动设置 include_thoughts
- **当** 设置思考预算且 budget != 0 时
- **且** 用户未显式设置 `include_thoughts`
- **则** The system SHALL将 `generationConfig.thinkingConfig.include_thoughts` 设为 `true`

#### Scenario: Gemini 3 自动设置 includeThoughts
- **当** 设置思考等级时
- **且** 用户未显式设置 `includeThoughts`
- **则** The system SHALL将 `generationConfig.thinkingConfig.includeThoughts` 设为 `true`
- **且** 若存在旧版 `include_thoughts` 字段则移除（Gemini 3 使用驼峰命名）

### 实现说明

```rust
fn is_gemini_3(model: &str) -> bool {
    model.contains("gemini-3")
}

fn is_gemini_3_flash(model: &str) -> bool {
    model.contains("gemini-3") && model.contains("flash")
}

fn budget_to_gemini3_level(model: &str, budget: i32) -> &'static str {
    if budget == -1 {
        return "high";  // auto -> high
    }
    if is_gemini_3_flash(model) {
        match budget {
            ..=512 => "minimal",
            ..=1024 => "low",
            ..=8192 => "medium",
            _ => "high",
        }
    } else {
        // Gemini 3 Pro：仅 low/high
        if budget <= 1024 { "low" } else { "high" }
    }
}

// 对于 Gemini 2.5
if !is_gemini_3(model) {
    body["generationConfig"]["thinkingConfig"]["thinkingBudget"] = budget;
    if !has_explicit_include_thoughts {
        body["generationConfig"]["thinkingConfig"]["include_thoughts"] = true;
    }
}

// 对于 Gemini 3
if is_gemini_3(model) {
    let level = budget_to_gemini3_level(model, budget);
    body["generationConfig"]["thinkingConfig"]["thinkingLevel"] = level;
    // 若存在则移除 thinkingBudget
    body["generationConfig"]["thinkingConfig"].remove("thinkingBudget");
    if !has_explicit_include_thoughts {
        body["generationConfig"]["thinkingConfig"]["includeThoughts"] = true;
    }
    // 清理旧版蛇形命名字段
    body["generationConfig"]["thinkingConfig"].remove("include_thoughts");
}
```

**关键点：**
- Gemini 2.5 使用 `thinkingBudget`（数值）+ `include_thoughts`（蛇形命名）
- Gemini 3 使用 `thinkingLevel`（字符串）+ `includeThoughts`（驼峰命名）
- 设置思考配置时，自动将 include_thoughts/includeThoughts 设为 `true`，除非用户显式指定
- `gemini-3-pro-preview` 等模型默认启用思考；仅在提供后缀时覆盖
