# Gemini 3 Flash 模型思考注入路径分析

本文档分析支持 `minimal` 等级的模型的思考配置注入路径。

## 模型信息

以 `gemini-3-flash-preview` 为例：

```rust
ModelInfo {
    id: "gemini-3-flash-preview",
    max_completion_tokens: 65536,
    thinking: Some(ThinkingSupport {
        min: 128,
        max: 32768,
        zero_allowed: false,
        dynamic_allowed: true,
        auto_budget: None,
        levels: Some(&["minimal", "low", "medium", "high"]),
    }),
}
```

**模型特征**：
- `levels = ["minimal", "low", "medium", "high"]` - 支持 minimal，**不包含 none 和 xhigh**
- `min = 128`, `max = 32768` - 有预算范围
- `zero_allowed = false` - 不允许 budget=0
- `dynamic_allowed = true` - 支持动态思考 (-1)
- `auto_budget = None` - 无自定义 auto_budget

## 等级到预算映射（通用）

| 等级 | 预算值 |
|------|--------|
| none | 0 |
| auto | -1 |
| minimal | 512 |
| low | 1024 |
| medium | 8192 |
| high | 24576 |
| xhigh | 32768 |

---

## 1. Gemini 3 Flash + Gemini 协议（原生协议）

模型有 levels，Gemini 协议根据用户输入类型选择输出格式：
- 等级后缀 → `thinkingLevel`（如果模型有 levels）
- 数值后缀 → `thinkingBudget`（尊重用户意图）

> **注意**：
> - `dynamic_allowed = true`，所以 `(auto)` 和 `(-1)` 直接透传为 `thinkingBudget: -1`
> - Gemini 3 模型有 levels，所以 `(none)` 和 `(0)` 返回 `Budget(0)` → `thinkingBudget: 0`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(0)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(auto)` | Gemini 协议特殊处理 → `ThinkingConfig::Budget(-1)` | `thinkingBudget: -1` |
| `(-1)` | 数值后缀 → 直接透传 | `thinkingBudget: -1` |
| `(minimal)` | → "minimal" | `thinkingLevel: "minimal"` |
| `(low)` | → "low" | `thinkingLevel: "low"` |
| `(medium)` | → "medium" | `thinkingLevel: "medium"` |
| `(high)` | → "high" | `thinkingLevel: "high"` |
| `(xhigh)` | levels 不包含 xhigh → clamp 到最高 → "high" | `thinkingLevel: "high"` |
| `(50)` | 数值后缀 → `clamp_budget(50, 128, 32768)` → 128 | `thinkingBudget: 128` |
| `(512)` | 数值后缀 → `clamp_budget(512, ...)` → 512 | `thinkingBudget: 512` |
| `(8192)` | 数值后缀 → 8192 | `thinkingBudget: 8192` |
| `(50000)` | 数值后缀 → `clamp_budget(50000, ...)` → 32768 | `thinkingBudget: 32768` |

---

## 2. Gemini 3 Flash + OpenAI 协议（跨协议）

需要 `ThinkingConfig::Effort`，注入 `reasoning_effort`

> **注意**：
> - `(none)` 和 `(0)` 返回 `Disabled` → `reasoning_effort: "none"`
> - levels 不包含 "none"，但 OpenAI 协议可以输出 "none"，由上游 API 处理
> - `(auto)` 和 `(-1)` 转换为 "medium"（OpenAI 不支持 auto）
> - levels 不包含 "xhigh"，向下 clamp 到 "high"

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(0)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(auto)` | → "auto" → OpenAI 不支持 → "medium" | `reasoning_effort: "medium"` |
| `(-1)` | `budget_to_effort(-1)` → "auto" → "medium" | `reasoning_effort: "medium"` |
| `(minimal)` | → "minimal" ✓ 在 levels | `reasoning_effort: "minimal"` |
| `(low)` | → "low" | `reasoning_effort: "low"` |
| `(medium)` | → "medium" | `reasoning_effort: "medium"` |
| `(high)` | → "high" | `reasoning_effort: "high"` |
| `(xhigh)` | levels 不包含 xhigh → clamp 到 "high" | `reasoning_effort: "high"` |
| `(50)` | `clamp_budget(50, ...)` → 128 → `budget_to_effort(128)` → "minimal" | `reasoning_effort: "minimal"` |
| `(512)` | → 512 → `budget_to_effort(512)` → "minimal" | `reasoning_effort: "minimal"` |
| `(1024)` | → 1024 → `budget_to_effort(1024)` → "low" | `reasoning_effort: "low"` |
| `(8192)` | → 8192 → `budget_to_effort(8192)` → "medium" | `reasoning_effort: "medium"` |
| `(24576)` | → 24576 → `budget_to_effort(24576)` → "high" | `reasoning_effort: "high"` |
| `(50000)` | `clamp_budget(50000, ...)` → 32768 → `budget_to_effort(32768)` → "xhigh" → clamp → "high" | `reasoning_effort: "high"` |

---

## 3. Gemini 3 Flash + Anthropic 协议（跨协议）

需要 `ThinkingConfig::Budget`，注入 `thinking.budget_tokens`

> **注意**：
> - `(none)` 和 `(0)` 返回 `Disabled` → `thinking: { type: "disabled" }`
> - Anthropic 协议不支持 `budget_tokens: -1`，但 `dynamic_allowed = true` 且模型有 budget range，所以使用 `auto_budget` 或 `(min+max)/2`
> - 由于 `auto_budget = None`，使用 `(128+32768)/2 = 16448`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(0)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `level_to_budget("auto")` → -1 → Anthropic 不支持 → `(128+32768)/2` | `budget_tokens: 16448` |
| `(-1)` | Anthropic 不支持 → `(128+32768)/2` | `budget_tokens: 16448` |
| `(minimal)` | `level_to_budget("minimal")` → 512 → 512 | `budget_tokens: 512` |
| `(low)` | `level_to_budget("low")` → 1024 → 1024 | `budget_tokens: 1024` |
| `(medium)` | `level_to_budget("medium")` → 8192 → 8192 | `budget_tokens: 8192` |
| `(high)` | `level_to_budget("high")` → 24576 → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `level_to_budget("xhigh")` → 32768 → 32768 | `budget_tokens: 32768` |
| `(50)` | `clamp_budget(50, 128, 32768)` → 128 | `budget_tokens: 128` |
| `(512)` | → 512 | `budget_tokens: 512` |
| `(8192)` | → 8192 | `budget_tokens: 8192` |
| `(50000)` | `clamp_budget(50000, ...)` → 32768 | `budget_tokens: 32768` |

---

## 与 Gemini 3 Pro Preview 的对比

| 特性 | gemini-3-flash-preview | gemini-3-pro-preview |
|------|------------------------|----------------------|
| levels | ["minimal", "low", "medium", "high"] | ["low", "high"] |
| min | 128 | 128 |
| max | 32768 | 32768 |
| dynamic_allowed | true | true |
| `(minimal)` 处理 | 直接支持 → "minimal" | clamp 到 "low" |
| `(medium)` 处理 | 直接支持 → "medium" | clamp 到 "high" |
| `(xhigh)` 处理 | clamp 到 "high" | clamp 到 "high" |

---

## 关键差异分析

### 1. `(minimal)` 后缀

- **Gemini 3 Flash**: levels 包含 "minimal" → 直接输出
- **Gemini 3 Pro**: levels 不包含 "minimal" → clamp 到 "low"
- **OpenAI 模型**: 取决于具体模型的 levels 配置

### 2. `(none)` 后缀

- **Gemini 3 Flash**: levels 不包含 "none"
  - Gemini 协议 → `thinkingBudget: 0`
  - OpenAI 协议 → `reasoning_effort: "none"`（透传给上游）
  - Anthropic 协议 → `thinking: { type: "disabled" }`

### 3. `(xhigh)` 后缀

- **Gemini 3 Flash**: levels 不包含 "xhigh" → clamp 到 "high"
- 数值 `(50000)` → clamp 到 max (32768) → 转换为 "xhigh" → clamp 到 "high"

### 4. 动态思考 `(auto)` / `(-1)`

- **Gemini 协议**: `dynamic_allowed = true` → 透传 -1
- **OpenAI 协议**: → "medium"
- **Anthropic 协议**: → `(min+max)/2 = 16448`
