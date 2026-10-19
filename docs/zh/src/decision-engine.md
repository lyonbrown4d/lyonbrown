# 可复用决策引擎

`decision-core` 是普通 Rust 库，负责生成建议和校验约束，不持有 Agent Session、数据库、Provider 凭据或 HTTP 服务。Agent 默认内嵌它；独立的 [AI Gateway](gateway.md) 使用同一接口选择模型。实际模型调用、权限与持久状态由调用方负责。

## 接口与结果

| Operation | 输出 |
| --- | --- |
| `RouteModel` | 任务域、复杂度、模型档位、所需能力及候选模型键 |
| `Classify` | 调用方给定标签中的分类与候选分数 |
| `Predicate` | 布尔判断及置信度 |
| `Choice` | 给定选项中的选择与分布 |
| `Score` | 给定量表内的数值 |
| `RecommendTools` | 已授权工具中的候选及分数 |
| `ContextAdvice` | 是否建议压缩及剩余 token 容量 |

请求携带 `request_id`、`operation`、`constraints`、可选策略版本和截止时间。结果状态为 `Accepted`、`Unknown`、`Degraded` 或 `Denied`；`Unknown` 不表示否定、低风险或允许。结果同时记录策略、后端、索引版本、原因码和耗时。置信度或相似度不能授予执行权限。

```rust
use decision_core::*;

# async fn example() -> Result<(), PolicyError> {
let engine = DecisionEngine::new(Policy::default(), Vec::new())?;
let request = DecisionRequest::new("example", Operation::Classify(ClassifyInput {
  text: "检查 Rust 代码".into(),
  labels: vec!["coding".into(), "chat".into()],
}));
let result = engine.decide(&request, &DecisionCancellation::new()).await;
# Ok(())
# }
```

可运行示例：`cargo run --locked -p decision-core --no-default-features --example embedded`。默认构建不加载权重、不访问网络，也不依赖 Agent Runtime。

## 决策顺序与硬约束

引擎先校验参数、策略版本和调用方提供的模型目录，再执行显式模型或任务绑定、规则、已配置后端与受约束的降级路径。规则使用配置中的词项；多个任务域同时匹配时不强行分类。模型目录来自调用方，不能从用户文本或分类器输出补充授权。

`local_only`、允许的模型键与档位、必需能力、上下文容量和单次预估费用约束在输出前复核。容量或费用未知时，不能假定满足对应限制。费用使用整数微美元，仅用于路由过滤，不是实际账单。显式指定或绑定的模型不合规时拒绝，不擅自换到其他 Provider。

规则路径能直接计算上下文建议并召回词项匹配的已授权工具。语义与结构化后端可扩展其余判断；未配置所需后端时返回 `Unknown`，不会伪造判断。后端错误、低置信度、非法输出和超时保留原因；取消会终止等待。无法确定分类时，只有调用方目录内仍符合硬约束的模型才可用于保守路由。

`replace_policy` 先验证完整不可变快照，再一次替换。单次请求固定读取同一份快照；修改内容必须使用新的 revision。库不保存长期任务绑定，调用方可通过 `pinned_model` 传入已有绑定。

## 语义与结构化后端

`decision-semantic` 使用可替换的 `EmbeddingProvider`、版本化示例和负样本，检查向量维度、有限值、阈值与首选候选差距。不确定时返回 Unknown。默认可由宿主注入 Embedding；`semantic-local` feature 提供 `fastembed` 本地适配。

本地适配只接受显式路径及 SHA-256 校验值：ONNX 模型、Tokenizer 配置和 ONNX Runtime 动态库。部署方须准备资产并确认许可证；加载不连接模型 Hub。ONNX Runtime 环境由进程共享，宿主需统一运行库配置，并复用首次初始化的环境。模型加载与推理使用阻塞工作线程，并发许可持续持有到实际计算结束；取消等待不会提前释放正在计算的槽位。模型与预处理版本、语料和阈值共同形成索引标识。

`decision-model` 将远程模型或已有本地 `InferenceBackend` 适配成结构化判断后端。它使用共享 `model-provider` 协议调用，不创建 Harness、不调用工具、不自动重试；输出必须符合对应 Operation 的类型、标签、选项与数值范围。`local_only` 请求不会发送给远程分类后端。该适配使用现有模型接入能力，不要求额外引入 Candle 判断模型。

## Agent 与 Gateway 的边界

Agent 的 `agent-decision` 保留工具、节点和审批相关的专用适配。每次 Run 默认调用共享核心，固定使用本次配置的主模型，并先保存 `model_route_decision`，再生成和保存工具执行计划、启动 Harness。该记录用于核对本次显式主模型绑定。分类建议不能绕过工作区限制、Worker 配对或人工审批。

Gateway 的 `router/auto` 使用其已配置模型目录调用同一个核心，按建议选择实际模型，并再次校验能力和本地范围。两种消费方式不共享 Agent Session，也不要求对方服务在线。

## 验证范围

核心测试覆盖规则、结构化输出、硬约束、取消、后端失败、版本替换和固定中英文回放语料。语义适配的确定性测试注入合成向量；它证明路由与拒绝行为，不代表真实多语言模型准确率。真实本地模型测试需显式提供资产配置，运行方式见[开发与验证](development.md)。

2026 年 10 月 19 日在 macOS arm64 上，显式提供固定版本的多语言 MiniLM ONNX 与 ONNX Runtime 资产，完成真实离线适配回放：4 条中英文已知输入分类正确，2 条未知输入拒判。校准语料与回放语料分开；这只是小样本适配验收，不是语言质量或性能基准。[验收记录](evidence/decision-semantic-offline.json)列出资产版本、阈值和范围。
