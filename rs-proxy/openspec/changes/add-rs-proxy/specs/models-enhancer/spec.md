## ADDED Requirements

### Requirement: 模型列表增强

The system SHALL 为模型列表响应添加思考等级变体。此功能与 CLIProxyAPI 不同，后者不包含变体。

**文件：** `src/models/enhancer.rs`

#### Scenario: OpenAI 模型端点
- **当** 以 OpenAI 请求头向 `/v1/models` 发起 GET 请求时
- **则** The system SHALL 转发到上游
- **且** 为每个支持思考的模型添加其支持的等级变体

#### Scenario: Anthropic 模型端点
- **当** 以 Anthropic 请求头向 `/v1/models` 发起 GET 请求时
- **则** The system SHALL 转发到上游
- **且** 为响应添加思考变体

#### Scenario: Gemini 模型端点
- **当** 向 `/v1beta/models` 发起 GET 请求时
- **则** The system SHALL 转发到上游
- **且** 为支持的模型添加思考变体

### Requirement: 等级变体生成规则

The system SHALL 根据模型的思考支持配置生成等级变体。

**文件：** `src/models/enhancer.rs`

#### Scenario: 模型有离散等级列表
- **当** 模型定义了 `thinking.levels`
- **则** The system SHALL 直接使用模型定义的等级列表生成变体
- **例如** 模型有 `levels=["low", "medium", "high"]`
- **则** 生成 `{model}(low)`, `{model}(medium)`, `{model}(high)`

#### Scenario: 模型使用数值预算范围
- **当** 模型未定义 `thinking.levels`
- **且** 模型定义了 `thinking.min` 和 `thinking.max`
- **则** The system SHALL 根据预算范围映射到等级列表
- **映射规则：**
  - 包含所有预算值在 `[min, max]` 范围内的等级
  - **向下覆盖：** 包含第一个预算 `<= min` 的等级
  - **向上覆盖：** 包含第一个预算 `>= max` 的等级

> **设计原理：**
> 返回的等级列表覆盖模型的完整预算范围。
> 边界等级可能略微超出模型范围，但请求时会钳制到模型支持的范围。
> 例如：模型 max=30000，返回 `xhigh`(32768)，请求时钳制到 30000。
> 这样用户可以使用熟悉的等级名称，而不需要知道精确的预算范围。

#### Scenario: 特殊等级处理
- **当** 生成等级变体时
- **则** The system SHALL 根据模型配置决定是否包含特殊等级：
  - `none`：仅当 `zero_allowed == true` 时包含
  - `auto`：仅当 `dynamic_allowed == true` 时包含

### 实现说明

```rust
use crate::models::registry::{get_model_info, ModelInfo, ThinkingSupport};
use crate::protocol::Protocol;

/// 模型信息（简化结构，实际根据协议响应格式定义）
#[derive(Clone)]
pub struct Model {
    pub id: String,
    // 其他字段根据协议响应格式添加...
}

/// 标准等级及其预算值（按预算升序排列）
const STANDARD_LEVELS: &[(&str, i32)] = &[
    ("none", 0),
    ("minimal", 512),
    ("low", 1024),
    ("medium", 8192),
    ("high", 24576),
    ("xhigh", 32768),
];

/// 为模型生成支持的等级列表
fn get_supported_levels(thinking: &ThinkingSupport) -> Vec<&'static str> {
    // 如果模型有离散等级，直接使用
    if let Some(levels) = thinking.levels {
        return levels.to_vec();
    }

    // 模型使用数值预算范围，映射到等级
    let mut supported = Vec::new();

    // 添加特殊等级
    if thinking.zero_allowed {
        supported.push("none");
    }
    if thinking.dynamic_allowed {
        supported.push("auto");
    }

    // 找到向下覆盖的等级：最大的 budget <= min 的等级
    // 反向遍历，第一个满足条件的就是最大的
    let mut lower_bound_idx = None;
    for (i, &(_, budget)) in STANDARD_LEVELS.iter().enumerate().rev() {
        if budget <= thinking.min && budget > 0 {  // 跳过 none（通过 zero_allowed 处理）
            lower_bound_idx = Some(i);
            break;
        }
    }

    // 找到向上覆盖的等级：最小的 budget >= max 的等级
    // 正向遍历，第一个满足条件的就是最小的
    let mut upper_bound_idx = None;
    for (i, &(_, budget)) in STANDARD_LEVELS.iter().enumerate() {
        if budget >= thinking.max {
            upper_bound_idx = Some(i);
            break;
        }
    }

    // 添加范围内的等级
    // 如果向下覆盖没找到（min 太小），从 minimal 开始
    // 如果向上覆盖没找到（max 超过所有等级），使用 xhigh
    let start = lower_bound_idx.unwrap_or(1);  // 1 = minimal
    let end = upper_bound_idx.unwrap_or(STANDARD_LEVELS.len() - 1);  // 5 = xhigh

    for i in start..=end {
        let (level, _) = STANDARD_LEVELS[i];
        if level != "none" {  // none 已通过 zero_allowed 处理
            supported.push(level);
        }
    }

    supported
}

/// 增强模型列表，添加思考等级变体
pub fn enhance_model_list(models: Vec<Model>) -> Vec<Model> {
    let mut enhanced = models.clone();

    for model in &models {
        if let Some(info) = get_model_info(&model.id) {
            if let Some(thinking) = &info.thinking {
                let levels = get_supported_levels(thinking);
                for level in levels {
                    enhanced.push(Model {
                        id: format!("{}({})", model.id, level),
                        ..model.clone()
                    });
                }
            }
        }
    }

    enhanced
}
```

**重要说明：** 这是 RS-Proxy 特有的功能。CLIProxyAPI 不会为模型列表添加变体。

### 等级映射示例

| 场景 | min | max | zero | dynamic | levels | 生成的变体 |
|------|-----|-----|------|---------|--------|-----------|
| 有离散等级 | - | - | - | - | ["minimal","low","medium","high"] | (minimal), (low), (medium), (high) |
| 预算范围（窄） | 1024 | 32768 | false | true | - | (auto), (low), (medium), (high), (xhigh) |
| 预算范围（宽） | 128 | 100000 | false | true | - | (auto), (minimal), (low), (medium), (high), (xhigh) |
| 有离散等级（稀疏） | 128 | 32768 | false | true | ["low","high"] | (low), (high) |

**映射过程示例（min=1024, max=100000）：**
1. 向下覆盖：第一个 <= 1024 的等级 → `low`(1024)
2. 向上覆盖：第一个 >= 100000 的等级 → 无（xhigh=32768 < 100000）→ 使用 `xhigh`
3. 范围 [low, xhigh] → `low`, `medium`, `high`, `xhigh`
4. dynamic_allowed=true → 添加 `auto`
5. 最终：`(auto)`, `(low)`, `(medium)`, `(high)`, `(xhigh)`
