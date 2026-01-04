# 思考配置注入路径文档

本文档详细说明所有模型类型、请求协议、后缀类型组合的处理路径和最终注入值。

## 设计决策：Gemini 协议特殊处理

> **⚠️ 重要设计决策**
>
> **1. 禁用思考处理 `(none)` / `(0)`**
>
> 当用户请求 `(none)` 等级或 `(0)` 预算时，语义是"禁用思考"。
>
> **协议差异处理**：
> - **Anthropic 协议**：返回 `ThinkingConfig::Disabled` → 注入 `thinking: { type: "disabled" }`
> - **OpenAI 协议**：返回 `ThinkingConfig::Disabled` → 注入 `reasoning_effort: "none"`
> - **Gemini 协议**：取决于模型类型：
>   - Gemini 3 模型（有 levels）：返回 `Budget(0)` → 注入 `thinkingBudget: 0`
>   - Gemini 2.5 模型（无 levels，原生 Gemini 模型）：clamp 到 `min`（如 128）
>   - 跨协议调用（如 Claude + Gemini 协议）：返回 `Budget(0)` → 注入 `thinkingBudget: 0`
>
> **2. 动态思考处理 `(auto)` / `(-1)`**
>
> 当用户请求 `(auto)` 等级或 `(-1)` 预算时，语义是"动态/自动思考"。
>
> **协议差异处理**：
> - **Gemini 协议**：返回 `ThinkingConfig::Budget(-1)` → 注入 `thinkingBudget: -1`
>   - 无论模型是否有 levels，`(auto)` 都直接透传为 -1
> - **OpenAI 协议**：转换为 `"medium"`（OpenAI 不支持 auto）
> - **Anthropic 协议**：使用 `auto_budget` 或 `level_to_budget("medium")` (8192)（Anthropic 不支持 -1）
>
> **3. 数值后缀处理**
>
> 当用户使用数值后缀（如 `(8192)`）时，Gemini 协议直接使用 `thinkingBudget`，尊重用户意图。

## 模型分类

> **注意**：以下为示例模型，不同模型的 `levels` 配置可能不同。
> 例如 `gpt-5.2` 支持 `["none","low","medium","high","xhigh"]`，而 `gpt-5.1` 只支持 `["none","low","medium","high"]`。

| 类型 | 模型示例 | `levels` | `min` | `max` | 特点 |
|------|---------|----------|-------|-------|------|
| Claude | claude-sonnet-4-5-20250929 | None | 1024 | 100000 | Budget-based |
| Gemini 2.5 | gemini-2.5-pro | None | 128 | 32768 | Budget-based |
| OpenAI | gpt-5.1 | ["none","low","medium","high"] | 0 | 0 | Level-based，无预算范围 |
| OpenAI | gpt-5.2 | ["none","low","medium","high","xhigh"] | 0 | 0 | Level-based，含 xhigh |
| Gemini 3 | gemini-3-pro-preview | ["low","high"] | 128 | 32768 | Level-based，有预算范围 |

## 等级到预算映射表（通用）

| 等级 | 预算值 |
|------|--------|
| none | 0 |
| auto | -1 |
| minimal | 512 |
| low | 1024 |
| medium | 8192 |
| high | 24576 |
| xhigh | 32768 |

## 预算到等级映射表（通用，双向对称）

> ✅ 此映射与等级到预算映射**双向对称**

| 预算范围 | 等级 | 说明 |
|---------|------|------|
| 0 | none | 禁用思考 |
| -1 | auto | 动态预算 |
| 1 ~ 512 | minimal | |
| 513 ~ 1024 | low | |
| 1025 ~ 8192 | medium | |
| 8193 ~ 24576 | high | |
| > 24576 | xhigh | |

> **注意**：对于有离散等级列表的模型，转换后会通过 `clamp_effort_to_levels` 钳制到模型支持的等级。

---

## 1. Claude 模型（Budget-based）

**模型特征**：`levels = None`, `min = 1024`, `max = 100000`, `zero_allowed = false`, `dynamic_allowed = false`, `auto_budget = 16384`

> **注意**：Claude API 不支持 `budget_tokens: -1`（动态预算），所以 `dynamic_allowed = false`。
> 当用户请求 `(auto)` 或 `(-1)` 时，使用 `auto_budget = 16384`。

### 1.1 Claude + Anthropic 协议（原生协议）

需要 `ThinkingConfig::Budget` 或 `ThinkingConfig::Disabled`，注入 `thinking`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `to_intent()` → `Dynamic` → `auto_budget(16384)` | `budget_tokens: 16384` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 512 → `clamp` → 1024 | `budget_tokens: 1024` |
| `(low)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 1024 → `clamp` → 1024 | `budget_tokens: 1024` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 8192 | `budget_tokens: 8192` |
| `(high)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 32768 | `budget_tokens: 32768` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | `to_intent()` → `Dynamic` → `auto_budget(16384)` | `budget_tokens: 16384` |
| `(500)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 1024 | `budget_tokens: 1024` |
| `(1024~100000)` | `to_intent()` → `Fixed(Budget)` → 直接使用 | `budget_tokens: {输入值}` |
| `(150000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 100000 | `budget_tokens: 100000` |

### 1.2 Claude + OpenAI 协议（跨协议）

需要 `ThinkingConfig::Effort` 或 `ThinkingConfig::Disabled`，注入 `reasoning_effort`

> **注意**：OpenAI 协议不支持 `reasoning_effort: "auto"`。
> - `(auto)` 等级后缀会直接转换为 `"medium"`
> - `(-1)` 数值后缀会透传给 `budget_to_effort(-1)` → `"auto"` → `"medium"`
> - 两者语义一致，都表示"自动/动态思考" → `"medium"`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(auto)` | `to_intent()` → `Dynamic` → `"medium"` | `reasoning_effort: "medium"` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "minimal"` |
| `(low)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "low"` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "medium"` |
| `(high)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "high"` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "xhigh"` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(-1)` | `to_intent()` → `Dynamic` → `"medium"` | `reasoning_effort: "medium"` |
| `(500)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 1024 → `budget_to_effort` → "low" | `reasoning_effort: "low"` |
| `(512)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 1024 → `budget_to_effort` → "low" | `reasoning_effort: "low"` |
| `(8192)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 8192 → `budget_to_effort` → "medium" | `reasoning_effort: "medium"` |
| `(24576)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "high" | `reasoning_effort: "high"` |
| `(32768)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "xhigh" | `reasoning_effort: "xhigh"` |
| `(100000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 100000 → `budget_to_effort` → "xhigh" | `reasoning_effort: "xhigh"` |

### 1.3 Claude + Gemini 协议（跨协议）

模型无 levels，需要 `ThinkingConfig::Budget`，注入 `thinkingBudget`

> **注意**：Gemini 协议对于 `(none)` 和 `(0)` 直接返回 `Budget(0)`，不走 clamp 逻辑。
> Gemini 协议支持 `thinkingBudget: -1`（动态思考），所以 `(-1)` 直接透传。

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → 跨协议 → `Budget(0)` | `thinkingBudget: 0` |
| `(0)` | `to_intent()` → `Disabled` → 跨协议 → `Budget(0)` | `thinkingBudget: 0` |
| `(auto)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 512 → `clamp` → 1024 | `thinkingBudget: 1024` |
| `(low)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 1024 → `clamp` → 1024 | `thinkingBudget: 1024` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 8192 | `thinkingBudget: 8192` |
| `(high)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 24576 | `thinkingBudget: 24576` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 32768 | `thinkingBudget: 32768` |
| `(-1)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(500)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 1024 | `thinkingBudget: 1024` |
| `(1024~100000)` | `to_intent()` → `Fixed(Budget)` → 直接使用 | `thinkingBudget: {输入值}` |
| `(150000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 100000 | `thinkingBudget: 100000` |

---

## 2. Gemini 2.5 模型（Budget-based）

**模型特征**：`levels = None`, `min = 128`, `max = 32768`, `zero_allowed = false/true`（取决于具体模型）, `dynamic_allowed = true`

以 `gemini-2.5-pro` 为例：`zero_allowed = false`

### 2.1 Gemini 2.5 + Gemini 协议（原生协议）

模型无 levels，需要 `ThinkingConfig::Budget`，注入 `thinkingBudget`

> **注意**：Gemini 2.5 没有 levels，`zero_allowed=false`，所以 `(none)` 和 `(0)` 需要 clamp 到 `min=128`。

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → 原生 Gemini 2.5 → `clamp` → 128 | `thinkingBudget: 128` |
| `(0)` | `to_intent()` → `Disabled` → 原生 Gemini 2.5 → `clamp` → 128 | `thinkingBudget: 128` |
| `(auto)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 512 | `thinkingBudget: 512` |
| `(low)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 1024 | `thinkingBudget: 1024` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 8192 | `thinkingBudget: 8192` |
| `(high)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 24576 | `thinkingBudget: 24576` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 32768 | `thinkingBudget: 32768` |
| `(-1)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(50)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 128 | `thinkingBudget: 128` |
| `(128~32768)` | `to_intent()` → `Fixed(Budget)` → 直接使用 | `thinkingBudget: {输入值}` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 32768 | `thinkingBudget: 32768` |

### 2.2 Gemini 2.5 + OpenAI 协议（跨协议）

需要 `ThinkingConfig::Effort` 或 `ThinkingConfig::Disabled`，注入 `reasoning_effort`

> **注意**：OpenAI 协议不支持 `reasoning_effort: "auto"`。
> - `(auto)` 等级后缀会直接转换为 `"medium"`
> - `(-1)` 数值后缀会透传给 `budget_to_effort(-1)` → `"auto"` → `"medium"`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(auto)` | `to_intent()` → `Dynamic` → `"medium"` | `reasoning_effort: "medium"` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "minimal"` |
| `(low)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "low"` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "medium"` |
| `(high)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "high"` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → 直接使用 | `reasoning_effort: "xhigh"` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(-1)` | `to_intent()` → `Dynamic` → `"medium"` | `reasoning_effort: "medium"` |
| `(50)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 128 → `budget_to_effort` → "minimal" | `reasoning_effort: "minimal"` |
| `(512)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 512 → `budget_to_effort` → "minimal" | `reasoning_effort: "minimal"` |
| `(8192)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "medium" | `reasoning_effort: "medium"` |
| `(24576)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "high" | `reasoning_effort: "high"` |
| `(32768)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "xhigh" | `reasoning_effort: "xhigh"` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 32768 → `budget_to_effort` → "xhigh" | `reasoning_effort: "xhigh"` |

### 2.3 Gemini 2.5 + Anthropic 协议（跨协议）

需要 `ThinkingConfig::Budget` 或 `ThinkingConfig::Disabled`，注入 `thinking.budget_tokens`

> **注意**：Anthropic 协议不支持 `budget_tokens: -1`（动态预算）。
> - `(auto)` 和 `(-1)` 会被转换为 `(min + max) / 2 = (128 + 32768) / 2 = 16448`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `to_intent()` → `Dynamic` → `(min+max)/2` → 16448 | `budget_tokens: 16448` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 512 | `budget_tokens: 512` |
| `(low)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 1024 | `budget_tokens: 1024` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 8192 | `budget_tokens: 8192` |
| `(high)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 32768 | `budget_tokens: 32768` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | `to_intent()` → `Dynamic` → `(min+max)/2` → 16448 | `budget_tokens: 16448` |
| `(50)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 128 | `budget_tokens: 128` |
| `(128~32768)` | `to_intent()` → `Fixed(Budget)` → 直接使用 | `budget_tokens: {输入值}` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 32768 | `budget_tokens: 32768` |

---

## 3. OpenAI 模型（Level-based，无预算范围）

**模型特征**：`levels = Some([...])`, `min = 0`, `max = 0`（无预算范围）

> **注意**：不同 OpenAI 模型的 levels 不同。
> - `gpt-5.1`：`["none", "low", "medium", "high"]`
> - `gpt-5.2`：`["none", "low", "medium", "high", "xhigh"]`
>
> 以下示例以 `gpt-5.1` 为例，`(xhigh)` 会被 clamp 到 `"high"`。
> 如果模型支持 `xhigh`（如 `gpt-5.2`），则直接使用 `"xhigh"`。

### 3.1 OpenAI + OpenAI 协议（原生协议）

需要 `ThinkingConfig::Effort`，注入 `reasoning_effort`

> **注意**：OpenAI 协议不支持 `reasoning_effort: "auto"`，`(auto)` 和 `(-1)` 都会被转换为 "medium"。

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(auto)` | `to_intent()` → `Dynamic` → `"medium"` → `clamp` → "medium" | `reasoning_effort: "medium"` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `clamp` → "low" | `reasoning_effort: "low"` |
| `(low)` | `to_intent()` → `Fixed(Level)` → "low" | `reasoning_effort: "low"` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → "medium" | `reasoning_effort: "medium"` |
| `(high)` | `to_intent()` → `Fixed(Level)` → "high" | `reasoning_effort: "high"` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `clamp` → "high" | `reasoning_effort: "high"` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(-1)` | `to_intent()` → `Dynamic` → `"medium"` → `clamp` → "medium" | `reasoning_effort: "medium"` |
| `(8192)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "medium" → `clamp` | `reasoning_effort: "medium"` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "xhigh" → `clamp` → "high" | `reasoning_effort: "high"` |

### 3.2 OpenAI + Anthropic 协议（跨协议）

需要 `ThinkingConfig::Budget` 或 `ThinkingConfig::Disabled`，注入 `thinking.budget_tokens`

> **注意**：
> - 模型无预算范围（max=0），不做 clamp
> - Anthropic 协议不支持 `budget_tokens: -1`，使用 `auto_budget` 或默认 `level_to_budget("medium")` (8192)

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `to_intent()` → `Dynamic` → `DEFAULT_MEDIUM_BUDGET` → 8192 | `budget_tokens: 8192` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 512 | `budget_tokens: 512` |
| `(low)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 1024 | `budget_tokens: 1024` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 8192 | `budget_tokens: 8192` |
| `(high)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 32768 | `budget_tokens: 32768` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | `to_intent()` → `Dynamic` → `DEFAULT_MEDIUM_BUDGET` → 8192 | `budget_tokens: 8192` |
| `(8192)` | `to_intent()` → `Fixed(Budget)` → 无 range 不 clamp | `budget_tokens: 8192` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → 无 range 不 clamp | `budget_tokens: 50000` |

### 3.3 OpenAI + Gemini 协议（跨协议）

模型有 levels，但 Gemini 协议根据用户输入类型选择输出格式：
- 等级后缀 → `thinkingLevel`
- 数值后缀 → `thinkingBudget`（尊重用户意图）

> **注意**：
> - `(none)` 和 `(0)` 直接返回 `Budget(0)`
> - 数值后缀直接使用 `thinkingBudget`，不转换为等级

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Budget(0)` | `thinkingBudget: 0` |
| `(0)` | `to_intent()` → `Disabled` → `Budget(0)` | `thinkingBudget: 0` |
| `(auto)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `clamp` → "low" | `thinkingLevel: "low"` |
| `(low)` | `to_intent()` → `Fixed(Level)` → "low" | `thinkingLevel: "low"` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → "medium" | `thinkingLevel: "medium"` |
| `(high)` | `to_intent()` → `Fixed(Level)` → "high" | `thinkingLevel: "high"` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `clamp` → "high" | `thinkingLevel: "high"` |
| `(-1)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(8192)` | `to_intent()` → `Fixed(Budget)` → 无 range 不 clamp | `thinkingBudget: 8192` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → 无 range 不 clamp | `thinkingBudget: 50000` |

---

## 4. Gemini 3 模型（Level-based，有预算范围）

**模型特征**：`levels = Some([...])`, `min > 0`, `max > 0`

以 `gemini-3-pro-preview` 为例：`levels = ["low", "high"]`, `min = 128`, `max = 32768`

### 4.1 Gemini 3 + Gemini 协议（原生协议）

模型有 levels，Gemini 协议根据用户输入类型选择输出格式：
- 等级后缀 → `thinkingLevel`
- 数值后缀 → `thinkingBudget`（尊重用户意图）

> **注意**：
> - `(none)` 和 `(0)` 直接返回 `Budget(0)`
> - 数值后缀直接使用 `thinkingBudget`，会进行 clamp

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Budget(0)` | `thinkingBudget: 0` |
| `(0)` | `to_intent()` → `Disabled` → `Budget(0)` | `thinkingBudget: 0` |
| `(auto)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `clamp` → "low" | `thinkingLevel: "low"` |
| `(low)` | `to_intent()` → `Fixed(Level)` → "low" | `thinkingLevel: "low"` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `clamp` → "high" | `thinkingLevel: "high"` |
| `(high)` | `to_intent()` → `Fixed(Level)` → "high" | `thinkingLevel: "high"` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `clamp` → "high" | `thinkingLevel: "high"` |
| `(-1)` | `to_intent()` → `Dynamic` → `Budget(-1)` | `thinkingBudget: -1` |
| `(50)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 128 | `thinkingBudget: 128` |
| `(500)` | `to_intent()` → `Fixed(Budget)` → 500 | `thinkingBudget: 500` |
| `(1024)` | `to_intent()` → `Fixed(Budget)` → 1024 | `thinkingBudget: 1024` |
| `(8192)` | `to_intent()` → `Fixed(Budget)` → 8192 | `thinkingBudget: 8192` |
| `(24576)` | `to_intent()` → `Fixed(Budget)` → 24576 | `thinkingBudget: 24576` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 32768 | `thinkingBudget: 32768` |

### 4.2 Gemini 3 + OpenAI 协议（跨协议）

需要 `ThinkingConfig::Effort` 或 `ThinkingConfig::Disabled`，注入 `reasoning_effort`

> **注意**：
> - `(none)` 和 `(0)` 直接返回 `Disabled` → `reasoning_effort: "none"`，让上游 API 决定如何处理
> - `clamp_effort_to_levels` 会先处理等级，"auto" 不在 levels 时回退到 "medium"
> - "medium" 再 clamp 到 ["low", "high"] → "high"
> - OpenAI 协议的 "auto" → "medium" 转换只对 levels 包含 "auto" 的模型生效

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(auto)` | `to_intent()` → `Dynamic` → `"medium"` → `clamp` → "high" | `reasoning_effort: "high"` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `clamp` → "low" | `reasoning_effort: "low"` |
| `(low)` | `to_intent()` → `Fixed(Level)` → "low" | `reasoning_effort: "low"` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `clamp` → "high" | `reasoning_effort: "high"` |
| `(high)` | `to_intent()` → `Fixed(Level)` → "high" | `reasoning_effort: "high"` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `clamp` → "high" | `reasoning_effort: "high"` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `reasoning_effort: "none"` |
| `(-1)` | `to_intent()` → `Dynamic` → `"medium"` → `clamp` → "high" | `reasoning_effort: "high"` |
| `(500)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 500 → `budget_to_effort` → "minimal" → `clamp` → "low" | `reasoning_effort: "low"` |
| `(8192)` | `to_intent()` → `Fixed(Budget)` → `budget_to_effort` → "medium" → `clamp` → "high" | `reasoning_effort: "high"` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 32768 → `budget_to_effort` → "xhigh" → `clamp` → "high" | `reasoning_effort: "high"` |

### 4.3 Gemini 3 + Anthropic 协议（跨协议）

需要 `ThinkingConfig::Budget` 或 `ThinkingConfig::Disabled`，注入 `thinking.budget_tokens`

> **注意**：Anthropic 协议不支持 `budget_tokens: -1`（动态预算）。
> - `(auto)` 和 `(-1)` 会被转换为 `(min + max) / 2 = (128 + 32768) / 2 = 16448`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `to_intent()` → `Dynamic` → `(min+max)/2` → 16448 | `budget_tokens: 16448` |
| `(minimal)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 512 → `clamp` → 512 | `budget_tokens: 512` |
| `(low)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 1024 | `budget_tokens: 1024` |
| `(medium)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 8192 | `budget_tokens: 8192` |
| `(high)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `to_intent()` → `Fixed(Level)` → `level_to_budget` → 32768 | `budget_tokens: 32768` |
| `(0)` | `to_intent()` → `Disabled` → `Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | `to_intent()` → `Dynamic` → `(min+max)/2` → 16448 | `budget_tokens: 16448` |
| `(50)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 128 | `budget_tokens: 128` |
| `(128~32768)` | `to_intent()` → `Fixed(Budget)` → 直接使用 | `budget_tokens: {输入值}` |
| `(50000)` | `to_intent()` → `Fixed(Budget)` → `clamp` → 32768 | `budget_tokens: 32768` |

---

## 处理流程总结

```
用户请求: model(suffix) + 请求协议

1. 解析后缀 (parse_model_suffix)
   ├─ 等级后缀 (high, low, ...) → ThinkingValue::Level
   └─ 数值后缀 (16384, ...) → ThinkingValue::Budget

2. 转换为意图 (to_intent)
   ├─ None (无后缀) → 直接透传
   ├─ (none) 或 (0) → ThinkingIntent::Disabled
   ├─ (auto) 或 (-1) → ThinkingIntent::Dynamic
   └─ 其他 → ThinkingIntent::Fixed(Level/Budget)

3. 查询模型信息
   ├─ 未知模型 + 有后缀 → 返回 400 错误
   ├─ 已知模型 + 无思考支持 → 去除后缀，透传
   └─ 已知模型 + 有思考支持 → 继续处理

4. 解析意图到配置 (resolve_intent_to_config)
   ├─ Disabled 意图:
   │   ├─ OpenAI/Anthropic → ThinkingConfig::Disabled
   │   └─ Gemini:
   │       ├─ 有 levels (Gemini 3) → Budget(0)
   │       ├─ 原生 Gemini 2.5 → clamp 到 min
   │       └─ 跨协议模型 → Budget(0)
   ├─ Dynamic 意图:
   │   ├─ Anthropic → auto_budget 或 (min+max)/2
   │   ├─ OpenAI → "medium" (+ clamp if has levels)
   │   └─ Gemini → Budget(-1)
   └─ Fixed 意图:
       ├─ Fixed(Level): 用户输入等级后缀，如 (high)、(medium)
       │   ├─ 验证: level_to_budget(level) → 无效则返回 400 错误
       │   │
       │   ├─ 情况 A: needs_effort = true
       │   │   触发条件: OpenAI 协议（始终）或 Gemini 协议 + 有 levels 的模型
       │   │   目标: 返回 Effort(等级字符串)
       │   │   │
       │   │   ├─ 模型有 levels 配置?
       │   │   │   ├─ 是 → clamp_effort_to_levels(level, levels)
       │   │   │   │       例: gpt-5.1(xhigh)，模型只支持 ["none","low","medium","high"]
       │   │   │   │           → clamp 到 "high"
       │   │   │   └─ 否 → 直接使用用户输入的 level（不 clamp）
       │   │   │           例: claude-sonnet-4-5(high) + OpenAI 协议
       │   │   │               → Claude 无 levels，直接用 "high"
       │   │   └─ 返回 Effort(level)
       │   │
       │   └─ 情况 B: needs_effort = false (needs_budget)
       │       触发条件: Anthropic 协议（始终）或 Gemini 协议 + 无 levels 的模型
       │       目标: 返回 Budget(数值)
       │       │
       │       ├─ level_to_budget(level) → 转换为数值
       │       │   例: "high" → 24576, "minimal" → 512
       │       │
       │       ├─ 模型有预算范围 (max > 0)?
       │       │   ├─ 是 → clamp_budget(budget, min, max, ...)
       │       │   │       例: gemini-2.5-pro(minimal)=512，模型 min=128
       │       │   │           → 512 在范围内，不变
       │       │   │       例: claude-sonnet-4-5(minimal)=512，模型 min=1024
       │       │   │           → clamp 到 1024
       │       │   └─ 否 → 直接使用 budget（无需 clamp）
       │       │           例: gpt-5.1(high) + Anthropic 协议
       │       │               → OpenAI 模型无预算范围 (max=0)，直接用 24576
       │       └─ 返回 Budget(budget)
       └─ Fixed(Budget): 用户输入数值后缀，如 (8192)、(16384)
           │
           ├─ Gemini 协议:
           │   触发条件: 请求使用 Gemini 协议
           │   目标: 返回 Budget(数值)，尊重用户的数值输入
           │   │
           │   ├─ 模型有预算范围 (max > 0)?
           │   │   ├─ 是 → clamp_budget(budget, min, max, ...)
           │   │   │       例: gemini-2.5-pro(50000)，max=32768
           │   │   │           → clamp 到 32768
           │   │   └─ 否 → 直接使用 budget（无需 clamp）
           │   │           例: gpt-5.1(8192) + Gemini 协议
           │   │               → OpenAI 模型无预算范围，直接用 8192
           │   └─ 返回 Budget(budget)
           │
           ├─ OpenAI 协议:
           │   触发条件: 请求使用 OpenAI 协议
           │   目标: 返回 Effort(等级字符串)，需要将数值转换为等级
           │   │
           │   ├─ 模型有预算范围 (max > 0)?
           │   │   ├─ 是 → clamp_budget(budget, min, max, ...)
           │   │   │       例: claude-sonnet-4-5(500) + OpenAI 协议
           │   │   │           → clamp 到 1024
           │   │   └─ 否 → 直接使用 budget
           │   │
           │   ├─ budget_to_effort(budget) → 转换为等级
           │   │   例: 8192 → "medium", 24576 → "high", 50000 → "xhigh"
           │   │
           │   ├─ 模型有 levels 配置?
           │   │   ├─ 是 → clamp_effort_to_levels(effort, levels)
           │   │   │       例: gpt-5.1(50000) → "xhigh" → 模型只支持到 "high"
           │   │   │           → clamp 到 "high"
           │   │   └─ 否 → 直接使用 effort
           │   │           例: claude-sonnet-4-5(8192) + OpenAI 协议
           │   │               → "medium"，Claude 无 levels，直接用
           │   └─ 返回 Effort(effort)
           │
           └─ Anthropic 协议:
               触发条件: 请求使用 Anthropic 协议
               目标: 返回 Budget(数值)
               │
               ├─ 模型有预算范围 (max > 0)?
               │   ├─ 是 → clamp_budget(budget, min, max, ...)
               │   │       例: gemini-2.5-pro(50000) + Anthropic 协议
               │   │           → clamp 到 32768
               │   │       例: claude-sonnet-4-5(500) + Anthropic 协议
               │   │           → clamp 到 1024
               │   └─ 否 → 直接使用 budget（无需 clamp）
               │           例: gpt-5.1(50000) + Anthropic 协议
               │               → OpenAI 模型无预算范围，直接用 50000
               └─ 返回 Budget(budget)

5. 注入到请求体 (inject_{openai,anthropic,gemini})
   ├─ Disabled:
   │   ├─ Anthropic → thinking: { type: "disabled" }
   │   └─ OpenAI → reasoning_effort: "none"
   ├─ Effort → reasoning_effort / thinkingLevel
   └─ Budget → thinking.budget_tokens / thinkingBudget
```

---

## 特殊值处理

| 值 | 含义 | 处理规则（意图分流） |
|----|------|---------------------|
| 0 | 禁用思考 | `to_intent()` → `Disabled` → OpenAI/Anthropic: `Disabled`；Gemini: `Budget(0)` 或 clamp 到 min |
| -1 | 动态预算 | `to_intent()` → `Dynamic` → Anthropic: `auto_budget` 或 `(min+max)/2`；Gemini: `Budget(-1)`；OpenAI: `"medium"` |
| < min | 低于最小值 | `clamp` 到 min |
| > max | 高于最大值 | `clamp` 到 max |

---

*文档生成时间：2026-01-03*
*对应代码：src/thinking/injector.rs, src/thinking/models.rs*
