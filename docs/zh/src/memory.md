# 长期记忆

长期记忆由 daemon 管理，保存偏好、事实和操作说明。关系数据库保存正文、状态、修订、来源、Run 绑定和索引作业；Qdrant 只保存可重建的向量与检索元数据。命中向量不等于记忆仍然有效，也不授予工具或节点权限。

本文依据当前工作区实现。记忆工具、分批重建、TUI 与 Desktop 管理入口尚未完成本轮编译、测试、实际运行或截图验收；验证范围见[开发与验证](development.md#验证范围)。

## 范围与状态

| 范围 | daemon 如何确定身份 |
| --- | --- |
| `user` | 当前 daemon 的持久节点身份 |
| `project` | 当前节点与 Session 中已保存的规范化工作区路径，对应数据库中的项目记录 |
| `session` | 当前节点拥有的持久 Session |

客户端只选择范围种类，不能提交任意范围 ID 扩大访问权限。不带 Session 的管理请求只能访问当前用户范围。项目范围保留 Session 原来保存的工作区身份，不通过重新解析移动的路径或重定向的符号链接转移权限。

记忆种类为 `preference`、`fact`、`procedure`，状态为 `proposed`、`active`、`superseded`、`rejected`、`deleted`。只有未过期的 `active` 记忆参与检索。显式用户操作可以激活记忆。模型的 `memory_propose` 只能创建或更新 `proposed` 记录，不能自行激活或修改已经激活的记录；没有不经工具调用的自动提取。

写入检查 `expected_revision`，新修订必须恰好增加 1。保存、编辑和激活请求使用独立的 UUID `request_id` 去重；相同标识换用不同内容会冲突。忘记请求只使用记忆 ID 与 `expected_revision` 校验，作业重试使用作业 ID 与状态校验。两者均不自动重试；结果未知时先查询记录或作业状态。删除后的身份不能恢复。忘记操作清空当前正文及历史修订的正文和来源，保存删除状态并创建向量删除作业。它不会撤回已经提供给模型的上下文，逻辑清空不等于数据库旧页、WAL、外部备份或日志副本的安全擦除。

## 配置检索

`memory.enabled` 默认 `true`，`context_bytes` 默认 `8192`，允许 `1024..=65536`。默认未配置 embedding，因此仅提供关系数据库中的管理操作，不发起 embedding 请求、不创建向量索引，也不生成占位向量。`enabled = false` 同样不启动检索与索引消费者，管理操作仍可用。

以下内容放在 daemon 的 `config.toml` 中，与[共享配置](quickstart.md)使用相同的覆盖顺序。地址、模型与维度必须与实际服务一致；该示例未执行。

```toml
[memory]
enabled = true
context_bytes = 8192

[memory.embedding]
id = "local-embedding"
model = "deployed-embedding-model"
dimensions = 768
api_base = "http://127.0.0.1:8081/v1"
# api_key_env = "MEMORY_EMBEDDING_API_KEY"

[memory.vector]
backend = "embedded"
# directory = "memory/custom-index"
```

embedding 复用 Rig 的 OpenAI 兼容 embedding 适配器与共享受限 HTTP 客户端，调用 `/embeddings` 接口，要求单条、指定维度且全部为有限数值的向量。`id`、模型、维度和规范化 API 地址共同确定配置修订；密钥轮换不改变该身份。凭据只通过 `api_key_env` 指向 `.env` 或进程环境，不写入记忆记录或状态输出。远程地址要求 HTTPS，回环地址可用 HTTP；地址不能包含凭据、查询参数或 fragment。`LYONBROWN_MEMORY_ENABLED` 可覆盖启用状态。

### 选择向量后端

| 后端 | 当前接线与限制 |
| --- | --- |
| `embedded` | 使用固定版本 `qdrant-edge` 0.9.0 的 `EdgeShard`，不需要独立 Qdrant 进程；默认目录为 daemon 数据目录下的 `memory/<配置修订>` |
| `qdrant` | 使用固定版本 `qdrant-client` 1.20.0，经 gRPC 连接 Qdrant Server；启动检查要求服务端为 `1.20.x` |

远端检索复用 Rig-Qdrant 0.46.0 的 `QdrantVectorStore::top_n` 与 `QdrantFilter`，传入已生成的查询向量与范围过滤条件，不重复调用 embedding。返回后仍回读关系数据库核对权威修订、有效状态和权限。

远端 upsert 与按记忆修订精确删除使用 Qdrant SDK 的单次写接口，保留调用方指定的稳定身份与结果未知语义。索引写入不能由框架随机生成新身份或自行重试。Rig 没有 Qdrant Edge 接口，embedded 后端继续通过 Edge SDK 访问同一种 Qdrant 引擎。

自定义 embedded 相对目录以 daemon 数据目录为基准；索引持有本机所有权锁。已有索引的 embedding 描述、维度、Cosine 距离或 WAL 配置不匹配时明确报错。嵌入式 SDK 的同步工作通过受限阻塞任务执行；这不改变关系数据库使用原生异步连接的要求。

embedding 描述在创建 shard 前原子写入。无法加载的残缺 shard 仍会明确报错，不会自动删除或重建；关系数据库中的记忆正文保持独立。

使用独立服务器时替换向量配置：

```toml
[memory.vector]
backend = "qdrant"
endpoint = "http://127.0.0.1:6334"
collection = "lyonbrown_memory"
# api_key_env = "MEMORY_QDRANT_API_KEY"
```

远端集合不存在时创建命名向量 `memory`；已有集合必须具有配置维度与 Cosine 距离。Qdrant 后端与关系数据库独立选择，SQLite、PostgreSQL 或 MySQL 都不会自动决定向量后端。embedding 描述变化会为当前授权范围登记新的分批重建。默认 embedded 目录包含配置修订，因此使用新的描述时会选择对应目录；自定义目录或远端集合仍须满足维度与描述检查，重建不会绕过不兼容检查或自动删除 shard。仅切换到空索引而保持相同 embedding 描述时，需要显式发起重建。向量索引就绪不表示 embedding 服务已经通过实际推理验收。

## 管理与索引作业

认证的 `MemoryService` 提供列表、详情、保存、忘记、搜索、作业列表、显式重试、状态查询，以及重建请求和分页进度。管理客户端使用 daemon 的身份与 Session 校验；它不直接连接数据库或 Qdrant。命令入口见[终端与桌面客户端](clients.md)。

每次记忆变更在同一关系数据库事务中保存当前记录、修订、事件、Outbox 和索引意图。`active` 修订产生 upsert 作业，其他状态产生 delete 作业。一份关系数据库只支持由一个逻辑 daemon 独占，多个 daemon 共用同库消费和恢复尚未支持。该 daemon 中只有一个消费者领取持久作业；领取时增加 attempt，完成提交再次核对 attempt。

作业状态为 `pending`、`running`、`completed`、`failed`、`uncertain`。启动恢复将未结束的 `running` 作业改为 `uncertain`，不自动重放。`failed` 和 `uncertain` 需要显式重试；元数据保存成功不等于向量已写入。索引写入的超时、取消或结果无法确认会保留未知状态。未配置 embedding 时，持久索引意图不会由后台消费者执行。

### 分批重建

重建先持久化请求，再按记忆 ID 顺序扫描权威记录，每批最多 100 条。每批在同一关系数据库事务中保存扫描游标、计数、新索引作业、事件和 Outbox；不把全部记忆加载到内存，也不在登记请求时直接写入 Qdrant。中断后从已提交游标继续扫描，已生成的索引作业仍遵循原有领取、attempt 校验和未知结果规则。

重建请求保存授权范围和完整 embedding 描述。daemon 启动消费者时检查用户范围；访问 Session 时检查当前用户、项目和会话范围。首次使用或描述变化会登记对应范围的新请求；只有同范围最新请求的描述相同时才复用，配置从 A 切到 B 再切回 A 也会重新扫描。重建子作业绑定目标描述，消费者不会用新的 embedding 执行旧描述的子作业。历史 Session 和项目在再次访问时登记，不在启动时越过范围检查扫描全部历史会话。显式重建可在描述未变时重新扫描当前范围，适用于索引目录丢失或索引需要重建的情况。

`pending` 表示扫描或生成的索引作业尚未全部完成；只有扫描结束且全部子作业为 `completed` 时，重建才变为 `completed`。失败或未知子作业保留原状态，需要按具体记忆查看作业并显式重试。若子作业绑定的 embedding 描述与当前配置不同，重试请求会明确拒绝；须切回对应配置处理，不能用新模型执行旧描述的作业。旧请求的未知作业不会被新的重建请求自动重放，也不会阻止新请求登记扫描任务。重建进度表示索引执行状态，不代表真实 embedding 模型质量或性能已验收。

### TUI 与 Desktop 入口

TUI 在本地记录页显示 `/memory` 的结果。已有的列表、搜索、创建、详情、编辑、激活、忘记和作业操作见[客户端命令](clients.md#记忆管理)。`/memory rebuild` 为当前授权范围提交一次显式重建，`/memory rebuilds [AFTER_ID]` 每页查询 20 条进度。请求超时或断线后先查询进度，不自动再次提交。

Desktop 从 `Settings → Memory` 进入管理页。无 Session 时显示用户范围；选择本机拥有的 Session 后增加项目与会话范围。页面支持分页列表、搜索有效记忆、新建偏好／事实／操作说明、编辑正文、显式激活或拒绝提议、永久忘记、查看和重试索引作业，以及重建当前范围。选中已有记录时，修改保留原范围与种类；提交时使用显示记录的修订校验。连接或 Session 改变会清空旧结果和输入，旧异步结果不能覆盖新范围。尚未完成原生 UI 运行和截图验收。

### 模型记忆工具

工具只会加入宿主已经允许的集合，分类和模型输出不能扩大集合或选择任意范围 ID。

| 工具 | 当前行为 |
| --- | --- |
| `memory_read` | 按 ID 读取当前授权范围内、仍为 `active` 且未过期的权威记录 |
| `memory_search` | 按授权范围和 embedding 描述检索，并逐条复核当前修订和状态 |
| `memory_propose` | 仅接受范围种类、记忆种类、正文和可选 ID／预期修订；Session、Run、来源事件与 `owner_epoch` 由宿主绑定，只能写入 `proposed` |

提议工具在写入事务中重新校验 Session 所有权、Run 和来源事件。记忆正文或模型提议不能授予工具、模型或节点权限，也不能替代人工审批。读取和检索失败会明确报告不可用，不返回占位记忆或虚构向量。

## 检索、复核与 Run 绑定

检索同时限定范围与 embedding 身份。每个向量命中都回读关系数据库，检查当前修订、`active` 状态、有效期和授权范围，并去重排序；已删除、过期或滞后的向量不能直接返回正文。

Run 准备阶段最多检索 16 条，检索预算为 5 秒；注入内容受 `context_bytes` 的 UTF-8 字节预算限制。可选检索不可用或超时可以略过，关系数据库读取失败则阻止继续请求模型。暴露选定正文前，daemon 在事务中再次核对当前记录、Session 所有权、`owner_epoch` 和活动 Run，保存记忆 ID、修订、内容摘要及 `memory_bound` 事件。复核发现修订已失效时略过本次上下文。

正文以明确标注为不可信参考数据的 JSON 进入 system prompt。它不能覆盖用户当前请求、改变工具策略或授权执行；绑定只记录本次使用的内容身份，不承诺记忆是真实事实。

实现依据：`crates/agent-config/src/memory.rs`、`crates/memory-core/`、`crates/agent-memory/`、`crates/agent-storage/src/memory/`、`crates/memory-qdrant/`、`apps/agentd/src/composition/memory.rs`、`apps/agentd/src/memory/`、`apps/agentd/src/memory_context.rs`、`apps/agentd/src/memory_worker.rs`、`apps/agent-cli/src/memory_command.rs` 与 `apps/agent-desktop/src/resources/memory/`。
