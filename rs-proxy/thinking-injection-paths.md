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
| `(none)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `level_to_budget("auto")` → -1 → `clamp_budget(-1, ..., auto_budget=16384)` → 16384 | `budget_tokens: 16384` |
| `(minimal)` | `level_to_budget("minimal")` → 512 → `clamp_budget(512, ...)` → 1024 | `budget_tokens: 1024` |
| `(low)` | `level_to_budget("low")` → 1024 → `clamp_budget` → 1024 | `budget_tokens: 1024` |
| `(medium)` | `level_to_budget("medium")` → 8192 → 8192 | `budget_tokens: 8192` |
| `(high)` | `level_to_budget("high")` → 24576 → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `level_to_budget("xhigh")` → 32768 → 32768 | `budget_tokens: 32768` |
| `(0)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | `clamp_budget(-1, ..., auto_budget=16384)` → 16384 | `budget_tokens: 16384` |
| `(500)` | `clamp_budget(500, ...)` → 1024 | `budget_tokens: 1024` |
| `(1024~100000)` | 直接使用 | `budget_tokens: {输入值}` |
| `(150000)` | `clamp_budget(150000, ...)` → 100000 | `budget_tokens: 100000` |

### 1.2 Claude + OpenAI 协议（跨协议）

需要 `ThinkingConfig::Effort` 或 `ThinkingConfig::Disabled`，注入 `reasoning_effort`

> **注意**：OpenAI 协议不支持 `reasoning_effort: "auto"`。
> - `(auto)` 等级后缀会直接转换为 `"medium"`
> - `(-1)` 数值后缀会透传给 `budget_to_effort(-1)` → `"auto"` → `"medium"`
> - 两者语义一致，都表示"自动/动态思考" → `"medium"`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(auto)` | OpenAI 不支持 auto → 转换为 "medium" | `reasoning_effort: "medium"` |
| `(minimal)` | 直接用 "minimal" | `reasoning_effort: "minimal"` |
| `(low)` | 直接用 "low" | `reasoning_effort: "low"` |
| `(medium)` | 直接用 "medium" | `reasoning_effort: "medium"` |
| `(high)` | 直接用 "high" | `reasoning_effort: "high"` |
| `(xhigh)` | 直接用 "xhigh" | `reasoning_effort: "xhigh"` |
| `(0)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(-1)` | 透传 -1 → `budget_to_effort(-1)` → "auto" → OpenAI 不支持 → "medium" | `reasoning_effort: "medium"` |
| `(500)` | `clamp_budget(500, ...)` → 1024 → `budget_to_effort(1024)` → "low" | `reasoning_effort: "low"` |
| `(512)` | `clamp_budget(512, ...)` → 1024 → `budget_to_effort(1024)` → "low" | `reasoning_effort: "low"` |
| `(8192)` | `clamp_budget(8192, ...)` → 8192 → `budget_to_effort(8192)` → "medium" | `reasoning_effort: "medium"` |
| `(24576)` | → "high" | `reasoning_effort: "high"` |
| `(32768)` | → "xhigh" | `reasoning_effort: "xhigh"` |
| `(100000)` | `clamp_budget(100000, ...)` → 100000 → "xhigh" | `reasoning_effort: "xhigh"` |

### 1.3 Claude + Gemini 协议（跨协议）

模型无 levels，需要 `ThinkingConfig::Budget`，注入 `thinkingBudget`

> **注意**：Gemini 协议对于 `(none)` 和 `(0)` 直接返回 `Budget(0)`，不走 clamp 逻辑。
> Gemini 协议支持 `thinkingBudget: -1`（动态思考），所以 `(-1)` 直接透传。

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(0)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(auto)` | Gemini 协议特殊处理 → `ThinkingConfig::Budget(-1)` | `thinkingBudget: -1` |
| `(minimal)` | `level_to_budget("minimal")` → 512 → `clamp_budget(512, ...)` → 1024 | `thinkingBudget: 1024` |
| `(low)` | `level_to_budget("low")` → 1024 → `clamp_budget` → 1024 | `thinkingBudget: 1024` |
| `(medium)` | `level_to_budget("medium")` → 8192 → 8192 | `thinkingBudget: 8192` |
| `(high)` | `level_to_budget("high")` → 24576 → 24576 | `thinkingBudget: 24576` |
| `(xhigh)` | `level_to_budget("xhigh")` → 32768 → 32768 | `thinkingBudget: 32768` |
| `(-1)` | Gemini 协议支持动态思考 → 直接透传 | `thinkingBudget: -1` |
| `(500)` | `clamp_budget(500, ...)` → 1024 | `thinkingBudget: 1024` |
| `(1024~100000)` | 直接使用 | `thinkingBudget: {输入值}` |
| `(150000)` | `clamp_budget(150000, ...)` → 100000 | `thinkingBudget: 100000` |

---

## 2. Gemini 2.5 模型（Budget-based）

**模型特征**：`levels = None`, `min = 128`, `max = 32768`, `zero_allowed = false/true`（取决于具体模型）, `dynamic_allowed = true`

以 `gemini-2.5-pro` 为例：`zero_allowed = false`

### 2.1 Gemini 2.5 + Gemini 协议（原生协议）

模型无 levels，需要 `ThinkingConfig::Budget`，注入 `thinkingBudget`

> **注意**：Gemini 2.5 没有 levels，`zero_allowed=false`，所以 `(none)` 和 `(0)` 需要 clamp 到 `min=128`。

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | `level_to_budget("none")` → 0 → `clamp_budget(0, 128, ...)` → 128 | `thinkingBudget: 128` |
| `(0)` | `clamp_budget(0, 128, ...)` → 128 | `thinkingBudget: 128` |
| `(auto)` | → -1（dynamic_allowed=true） | `thinkingBudget: -1` |
| `(minimal)` | → 512 → 512 | `thinkingBudget: 512` |
| `(low)` | → 1024 → 1024 | `thinkingBudget: 1024` |
| `(medium)` | → 8192 → 8192 | `thinkingBudget: 8192` |
| `(high)` | → 24576 → 24576 | `thinkingBudget: 24576` |
| `(xhigh)` | → 32768 → 32768 | `thinkingBudget: 32768` |
| `(-1)` | -1（dynamic_allowed=true） | `thinkingBudget: -1` |
| `(50)` | `clamp_budget(50, ...)` → 128 | `thinkingBudget: 128` |
| `(128~32768)` | 直接使用 | `thinkingBudget: {输入值}` |
| `(50000)` | `clamp_budget(50000, ...)` → 32768 | `thinkingBudget: 32768` |

### 2.2 Gemini 2.5 + OpenAI 协议（跨协议）

需要 `ThinkingConfig::Effort` 或 `ThinkingConfig::Disabled`，注入 `reasoning_effort`

> **注意**：OpenAI 协议不支持 `reasoning_effort: "auto"`。
> - `(auto)` 等级后缀会直接转换为 `"medium"`
> - `(-1)` 数值后缀会透传给 `budget_to_effort(-1)` → `"auto"` → `"medium"`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(auto)` | OpenAI 不支持 auto → 转换为 "medium" | `reasoning_effort: "medium"` |
| `(minimal)` | 直接用 "minimal" | `reasoning_effort: "minimal"` |
| `(low)` | 直接用 "low" | `reasoning_effort: "low"` |
| `(medium)` | 直接用 "medium" | `reasoning_effort: "medium"` |
| `(high)` | 直接用 "high" | `reasoning_effort: "high"` |
| `(xhigh)` | 直接用 "xhigh" | `reasoning_effort: "xhigh"` |
| `(0)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(-1)` | 透传 -1 → `budget_to_effort(-1)` → "auto" → OpenAI 不支持 → "medium" | `reasoning_effort: "medium"` |
| `(50)` | `clamp_budget(50, ...)` → 128 → `budget_to_effort(128)` → "minimal" | `reasoning_effort: "minimal"` |
| `(512)` | `clamp_budget(512, ...)` → 512 → `budget_to_effort(512)` → "minimal" | `reasoning_effort: "minimal"` |
| `(8192)` | `clamp_budget(8192, ...)` → 8192 → `budget_to_effort(8192)` → "medium" | `reasoning_effort: "medium"` |
| `(24576)` | `clamp_budget(24576, ...)` → 24576 → "high" | `reasoning_effort: "high"` |
| `(32768)` | `clamp_budget(32768, ...)` → 32768 → "xhigh" | `reasoning_effort: "xhigh"` |
| `(50000)` | `clamp_budget(50000, ...)` → 32768 → "xhigh" | `reasoning_effort: "xhigh"` |

### 2.3 Gemini 2.5 + Anthropic 协议（跨协议）

需要 `ThinkingConfig::Budget` 或 `ThinkingConfig::Disabled`，注入 `thinking.budget_tokens`

> **注意**：Anthropic 协议不支持 `budget_tokens: -1`（动态预算）。
> - `(auto)` 和 `(-1)` 会被转换为 `(min + max) / 2 = (128 + 32768) / 2 = 16448`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `level_to_budget("auto")` → -1 → Anthropic 不支持 → `(128+32768)/2` → 16448 | `budget_tokens: 16448` |
| `(minimal)` | `level_to_budget("minimal")` → 512 → 512 | `budget_tokens: 512` |
| `(low)` | `level_to_budget("low")` → 1024 → 1024 | `budget_tokens: 1024` |
| `(medium)` | `level_to_budget("medium")` → 8192 → 8192 | `budget_tokens: 8192` |
| `(high)` | `level_to_budget("high")` → 24576 → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `level_to_budget("xhigh")` → 32768 → 32768 | `budget_tokens: 32768` |
| `(0)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | Anthropic 不支持 → `(128+32768)/2` → 16448 | `budget_tokens: 16448` |
| `(50)` | `clamp_budget(50, ...)` → 128 | `budget_tokens: 128` |
| `(128~32768)` | 直接使用 | `budget_tokens: {输入值}` |
| `(50000)` | `clamp_budget(50000, ...)` → 32768 | `budget_tokens: 32768` |

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
| `(none)` | `clamp_effort_to_levels("none", levels)` → "none" | `reasoning_effort: "none"` |
| `(auto)` | `clamp_effort_to_levels("auto", levels)` → levels 无 auto → 回退到 "medium" | `reasoning_effort: "medium"` |
| `(minimal)` | `clamp_effort_to_levels("minimal", levels)` → 不在列表 → 向上 clamp → "low" | `reasoning_effort: "low"` |
| `(low)` | → "low" | `reasoning_effort: "low"` |
| `(medium)` | → "medium" | `reasoning_effort: "medium"` |
| `(high)` | → "high" | `reasoning_effort: "high"` |
| `(xhigh)` | `clamp_effort_to_levels("xhigh", levels)` → 向上无更高 → 返回最高 "high" | `reasoning_effort: "high"` |
| `(0)` | 无预算范围不 clamp → `budget_to_effort(0)` → "none" → clamp → "none" | `reasoning_effort: "none"` |
| `(-1)` | 无预算范围不 clamp → `budget_to_effort(-1)` → "auto" → OpenAI 不支持 → "medium" | `reasoning_effort: "medium"` |
| `(8192)` | `budget_to_effort(8192)` → "medium" → clamp → "medium" | `reasoning_effort: "medium"` |
| `(50000)` | `budget_to_effort(50000)` → "xhigh" → clamp → "high" | `reasoning_effort: "high"` |

### 3.2 OpenAI + Anthropic 协议（跨协议）

需要 `ThinkingConfig::Budget` 或 `ThinkingConfig::Disabled`，注入 `thinking.budget_tokens`

> **注意**：
> - 模型无预算范围（max=0），不做 clamp
> - Anthropic 协议不支持 `budget_tokens: -1`，使用 `auto_budget` 或默认 `level_to_budget("medium")` (8192)

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `level_to_budget("auto")` → -1 → Anthropic 不支持 → `auto_budget` 或 `level_to_budget("medium")` | `budget_tokens: 8192` |
| `(minimal)` | `level_to_budget("minimal")` → 512 | `budget_tokens: 512` |
| `(low)` | `level_to_budget("low")` → 1024 | `budget_tokens: 1024` |
| `(medium)` | `level_to_budget("medium")` → 8192 | `budget_tokens: 8192` |
| `(high)` | `level_to_budget("high")` → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `level_to_budget("xhigh")` → 32768 | `budget_tokens: 32768` |
| `(0)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | Anthropic 不支持 → `auto_budget` 或 `level_to_budget("medium")` | `budget_tokens: 8192` |
| `(8192)` | 直接使用（无 range 不 clamp） | `budget_tokens: 8192` |
| `(50000)` | 直接使用（无 range 不 clamp） | `budget_tokens: 50000` |

### 3.3 OpenAI + Gemini 协议（跨协议）

模型有 levels，但 Gemini 协议根据用户输入类型选择输出格式：
- 等级后缀 → `thinkingLevel`
- 数值后缀 → `thinkingBudget`（尊重用户意图）

> **注意**：
> - `(none)` 和 `(0)` 直接返回 `Budget(0)`
> - 数值后缀直接使用 `thinkingBudget`，不转换为等级

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(0)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(auto)` | Gemini 协议特殊处理 → `ThinkingConfig::Budget(-1)` | `thinkingBudget: -1` |
| `(minimal)` | `clamp_effort_to_levels("minimal", levels)` → 向上 clamp → "low" | `thinkingLevel: "low"` |
| `(low)` | → "low" | `thinkingLevel: "low"` |
| `(medium)` | → "medium" | `thinkingLevel: "medium"` |
| `(high)` | → "high" | `thinkingLevel: "high"` |
| `(xhigh)` | `clamp_effort_to_levels("xhigh", levels)` → 向上无更高 → 返回最高 "high" | `thinkingLevel: "high"` |
| `(-1)` | 数值后缀 → 直接使用 | `thinkingBudget: -1` |
| `(8192)` | 数值后缀 → 直接使用 | `thinkingBudget: 8192` |
| `(50000)` | 数值后缀 → 直接使用 | `thinkingBudget: 50000` |

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
| `(none)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(0)` | → `ThinkingConfig::Budget(0)` | `thinkingBudget: 0` |
| `(auto)` | Gemini 协议特殊处理 → `ThinkingConfig::Budget(-1)` | `thinkingBudget: -1` |
| `(minimal)` | → 向上 clamp → "low" | `thinkingLevel: "low"` |
| `(low)` | → "low" | `thinkingLevel: "low"` |
| `(medium)` | → 不在列表 → 向上 clamp → "high" | `thinkingLevel: "high"` |
| `(high)` | → "high" | `thinkingLevel: "high"` |
| `(xhigh)` | → 向上无更高 → 返回最高 "high" | `thinkingLevel: "high"` |
| `(-1)` | 数值后缀 → -1（dynamic_allowed=true） | `thinkingBudget: -1` |
| `(50)` | 数值后缀 → `clamp_budget(50, ...)` → 128 | `thinkingBudget: 128` |
| `(500)` | 数值后缀 → 500 | `thinkingBudget: 500` |
| `(1024)` | 数值后缀 → 1024 | `thinkingBudget: 1024` |
| `(8192)` | 数值后缀 → 8192 | `thinkingBudget: 8192` |
| `(24576)` | 数值后缀 → 24576 | `thinkingBudget: 24576` |
| `(50000)` | 数值后缀 → `clamp_budget(50000, ...)` → 32768 | `thinkingBudget: 32768` |

### 4.2 Gemini 3 + OpenAI 协议（跨协议）

需要 `ThinkingConfig::Effort` 或 `ThinkingConfig::Disabled`，注入 `reasoning_effort`

> **注意**：
> - `(none)` 和 `(0)` 直接返回 `Disabled` → `reasoning_effort: "none"`，让上游 API 决定如何处理
> - `clamp_effort_to_levels` 会先处理等级，"auto" 不在 levels 时回退到 "medium"
> - "medium" 再 clamp 到 ["low", "high"] → "high"
> - OpenAI 协议的 "auto" → "medium" 转换只对 levels 包含 "auto" 的模型生效

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(auto)` | levels 无 auto → 回退到 "medium" → clamp → "high" | `reasoning_effort: "high"` |
| `(minimal)` | → 向上 clamp → "low" | `reasoning_effort: "low"` |
| `(low)` | → "low" | `reasoning_effort: "low"` |
| `(medium)` | → clamp → "high" | `reasoning_effort: "high"` |
| `(high)` | → "high" | `reasoning_effort: "high"` |
| `(xhigh)` | → 向上无更高 → 返回最高 "high" | `reasoning_effort: "high"` |
| `(0)` | → `ThinkingConfig::Disabled` | `reasoning_effort: "none"` |
| `(-1)` | `budget_to_effort(-1)` → "auto" → levels 无 auto → 回退 "medium" → clamp → "high" | `reasoning_effort: "high"` |
| `(500)` | `clamp_budget(500, ...)` → 500 → `budget_to_effort(500)` → "minimal" → clamp → "low" | `reasoning_effort: "low"` |
| `(8192)` | `clamp_budget(8192, ...)` → 8192 → `budget_to_effort(8192)` → "medium" → clamp → "high" | `reasoning_effort: "high"` |
| `(50000)` | `clamp_budget(50000, ...)` → 32768 → `budget_to_effort(32768)` → "xhigh" → clamp → "high" | `reasoning_effort: "high"` |

### 4.3 Gemini 3 + Anthropic 协议（跨协议）

需要 `ThinkingConfig::Budget` 或 `ThinkingConfig::Disabled`，注入 `thinking.budget_tokens`

> **注意**：Anthropic 协议不支持 `budget_tokens: -1`（动态预算）。
> - `(auto)` 和 `(-1)` 会被转换为 `(min + max) / 2 = (128 + 32768) / 2 = 16448`

| 后缀 | 处理路径 | 最终值 |
|------|---------|--------|
| `(none)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(auto)` | `level_to_budget("auto")` → -1 → Anthropic 不支持 → `(128+32768)/2` → 16448 | `budget_tokens: 16448` |
| `(minimal)` | `level_to_budget("minimal")` → 512 → `clamp_budget(512, 128, 32768, ...)` → 512 | `budget_tokens: 512` |
| `(low)` | `level_to_budget("low")` → 1024 → 1024 | `budget_tokens: 1024` |
| `(medium)` | `level_to_budget("medium")` → 8192 → 8192 | `budget_tokens: 8192` |
| `(high)` | `level_to_budget("high")` → 24576 → 24576 | `budget_tokens: 24576` |
| `(xhigh)` | `level_to_budget("xhigh")` → 32768 → 32768 | `budget_tokens: 32768` |
| `(0)` | → `ThinkingConfig::Disabled` | `thinking: { type: "disabled" }` |
| `(-1)` | Anthropic 不支持 → `(128+32768)/2` → 16448 | `budget_tokens: 16448` |
| `(50)` | `clamp_budget(50, ...)` → 128 | `budget_tokens: 128` |
| `(128~32768)` | 直接使用 | `budget_tokens: {输入值}` |
| `(50000)` | `clamp_budget(50000, ...)` → 32768 | `budget_tokens: 32768` |

---

## 处理流程总结

```
用户请求: model(suffix) + 请求协议

1. 解析后缀
   ├─ 等级后缀 (high, low, ...) → ThinkingValue::Level
   └─ 数值后缀 (16384, ...) → ThinkingValue::Budget

2. 查询模型信息
   ├─ 未知模型 + 有后缀 → 返回 400 错误
   ├─ 已知模型 + 无思考支持 → 去除后缀，透传
   └─ 已知模型 + 有思考支持 → 继续处理

3. 禁用思考检查（协议差异处理）
   ├─ OpenAI/Anthropic 协议:
   │   ├─ level == "none" → ThinkingConfig::Disabled
   │   └─ budget == 0 → ThinkingConfig::Disabled
   └─ Gemini 协议:
       ├─ level == "none" → ThinkingConfig::Budget(0)
       ├─ level == "auto" → ThinkingConfig::Budget(-1)
       └─ budget == 0 → ThinkingConfig::Budget(0)

4. 确定协议需求
   ├─ OpenAI 协议 → needs_effort = true
   ├─ Anthropic 协议 → needs_effort = false
   └─ Gemini 协议 → needs_effort = 模型有 levels（仅对等级后缀生效）

5. 确定是否有预算范围
   └─ has_budget_range = thinking.max > 0

6. 转换和钳制
   ├─ 等级输入 + needs_effort:
   │   ├─ 有 levels → clamp_effort_to_levels → Effort
   │   └─ 无 levels → 直接使用 → Effort
   ├─ 等级输入 + needs_budget:
   │   ├─ level_to_budget → budget
   │   ├─ has_range → clamp_budget → Budget
   │   └─ 无 range → 直接使用 → Budget
   ├─ 数值输入 + Gemini 协议:
   │   └─ clamp_budget（如有 range）→ Budget（尊重用户意图）
   ├─ 数值输入 + OpenAI 协议:
   │   ├─ has_range → clamp_budget → clamped
   │   ├─ budget_to_effort(clamped) → effort
   │   ├─ 有 levels → clamp_effort_to_levels → Effort
   │   └─ 无 levels → 直接使用 → Effort
   └─ 数值输入 + Anthropic 协议:
       ├─ has_range → clamp_budget → Budget
       └─ 无 range → 直接使用 → Budget

7. OpenAI 协议 auto 转换（needs_effort=true 时）
   └─ 如果最终 effort == "auto" → 转换为 "medium"
   （无论是等级后缀 (auto) 还是数值后缀 (-1)，无论模型是否支持 auto）

8. 注入到请求体
   ├─ Disabled (仅 OpenAI/Anthropic):
   │   ├─ Anthropic → thinking: { type: "disabled" }
   │   └─ OpenAI → reasoning_effort: "none"
   ├─ Effort → reasoning_effort / thinkingLevel
   └─ Budget → thinking.budget_tokens / thinkingBudget
```

---

## 特殊值处理

| 值 | 含义 | 处理规则 |
|----|------|----------|
| 0 | 禁用思考 | OpenAI/Anthropic → `Disabled`；Gemini → `Budget(0)` |
| -1 | 动态预算 | Anthropic 不支持 → 使用 `auto_budget` 或 `(min+max)/2`；Gemini → 依据 `dynamic_allowed`；OpenAI → 转为 "auto" → "medium" |
| < min | 低于最小值 | clamp 到 min |
| > max | 高于最大值 | clamp 到 max |

---

*文档生成时间：2026-01-03*
*对应代码：src/thinking/injector.rs, src/thinking/models.rs*
