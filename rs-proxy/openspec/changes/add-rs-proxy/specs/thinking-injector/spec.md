## ADDED Requirements

### Requirement: 统一思考注入入口

The system SHALL 提供统一的思考配置注入入口，协调解析、验证、映射和注入流程。

**文件：** `src/thinking/injector.rs`

#### Scenario: 注入流程
- **当** 收到包含模型名称的 API 请求时
- **则** The system SHALL 执行以下流程：
  1. 调用 parser 解析模型后缀（如 `model(high)` → `model` + `high`）
  2. 检查基础模型是否在注册表中
  3. 检查模型是否支持思考配置
  4. 根据后缀类型（等级/数值）进行映射和钳制
  5. 根据协议类型注入对应的思考字段

### Requirement: 未知模型错误处理

The system SHALL 对未知模型带思考后缀返回 HTTP 400 错误。

**文件：** `src/thinking/injector.rs`

#### Scenario: 未知模型带思考后缀
- **当** 模型名称包含思考后缀（如 `unknown-model(high)`）
- **且** 基础模型不在注册表中
- **则** The system SHALL 返回 HTTP 400 错误
- **且** 错误信息应说明模型未知

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 允许未知模型使用思考后缀并采用回退行为。
> RS-Proxy 返回错误，确保行为可预测。

#### Scenario: 已知模型不支持思考
- **当** 模型名称包含思考后缀
- **且** 基础模型在注册表中但不支持思考
- **则** The system SHALL 去除后缀并使用基础模型名称
- **且** 不注入任何思考字段

#### Scenario: 空括号处理
- **当** 模型名称包含空括号（如 `model()`）
- **则** The system SHALL 去除空括号
- **且** 使用基础模型名称，不注入思考配置

### Requirement: 协议分发

The system SHALL 根据检测到的协议类型分发到对应的注入逻辑。

**文件：** `src/thinking/injector.rs`

#### Scenario: 协议特定注入
- **当** 思考配置已解析和映射完成
- **则** The system SHALL 根据协议类型调用对应的注入函数：
  - OpenAI → 设置 `reasoning_effort` 或 `reasoning.effort`
  - Anthropic → 设置 `thinking.type` + `thinking.budget_tokens`
  - Gemini → 设置 `thinkingBudget` 或 `thinkingLevel`

### Requirement: 数值预算等级钳制

The system SHALL 在数值预算转换为等级后，将等级钳制到模型支持的范围。

**文件：** `src/thinking/injector.rs`

#### Scenario: 转换后等级不在支持列表
- **当** 数值预算（如 `8000`）转换为等级（如 `medium`）
- **且** 模型只支持离散等级（如 `["low", "high"]`）
- **且** 转换后的等级不在支持列表中
- **则** The system SHALL 向上 clamp 到最近的支持等级（如 `medium` → `high`）

> **⚠️ 设计决策 - 与 CLIProxyAPI 不同：**
> CLIProxyAPI 不验证转换后的等级，依赖后续 `ValidateThinkingConfig` 返回 400。
> RS-Proxy 主动 clamp，提供更好的用户体验。

### 实现说明

```rust
use crate::models::registry::{get_model_info, model_supports_thinking};
use crate::protocol::Protocol;
use crate::thinking::parser::parse_model_suffix;
use crate::thinking::models::{level_to_budget, budget_to_effort_for_model, clamp_budget};

/// 思考注入结果
pub enum InjectionResult {
    /// 成功注入，返回修改后的请求体
    Injected(serde_json::Value),
    /// 无需注入（模型不支持或无后缀）
    PassThrough(serde_json::Value),
    /// 错误（未知模型带后缀）
    Error(InjectionError),
}

/// 注入错误类型
pub struct InjectionError {
    pub status: u16,
    pub message: String,
}

/// 统一注入入口
pub fn inject_thinking_config(
    body: serde_json::Value,
    model_with_suffix: &str,
    protocol: Protocol,
) -> InjectionResult {
    // 1. 解析后缀
    let (base_model, suffix) = parse_model_suffix(model_with_suffix);

    // 2. 无后缀或空后缀，直接透传
    let suffix = match suffix {
        Some(s) if !s.is_empty() => s,
        _ => return InjectionResult::PassThrough(body),
    };

    // 3. 检查模型是否已知
    let model_info = match get_model_info(&base_model) {
        Some(info) => info,
        None => return InjectionResult::Error(InjectionError {
            status: 400,
            message: format!("unknown model with thinking suffix: {}", model_with_suffix),
        }),
    };

    // 4. 检查模型是否支持思考
    if model_info.thinking.is_none() {
        // 已知模型但不支持思考，去除后缀透传
        let mut body = body;
        body["model"] = serde_json::Value::String(base_model);
        return InjectionResult::PassThrough(body);
    }

    // 5. 解析后缀值（数值或等级）
    let thinking_config = resolve_thinking_config(&suffix, model_info);

    // 6. 根据协议注入
    let injected = match protocol {
        Protocol::OpenAI => inject_openai(body, &base_model, thinking_config),
        Protocol::Anthropic => inject_anthropic(body, &base_model, thinking_config),
        Protocol::Gemini => inject_gemini(body, &base_model, thinking_config),
    };

    InjectionResult::Injected(injected)
}

/// 解析思考配置，处理数值/等级转换和钳制
fn resolve_thinking_config(suffix: &str, model_info: &ModelInfo) -> ThinkingConfig {
    // 实现细节：
    // - 数值后缀：clamp 到模型范围，必要时转换为等级
    // - 等级后缀：验证或向上 clamp 到支持的等级
    todo!()
}
```

### 模块依赖关系

```
thinking/injector.rs
    ├── thinking/parser.rs      (解析后缀)
    ├── thinking/models.rs      (等级/预算映射)
    ├── models/registry.rs      (模型查询)
    └── protocol/*.rs           (协议特定注入)
```
