# 本地运行

先准备根 `Cargo.toml` 要求的 Rust 工具链。模型、原生图形和浏览器 SDK 的前置条件取决于启用的能力；缺少原生依赖时不能仅凭 crate 名称认定该能力可用。

## 启动开发环境

在仓库根目录直接使用 Cargo 选择客户端，无需安装额外任务工具：

```sh
cargo dev tui
# 或启动桌面客户端
cargo dev desktop
```

命令自动构建、启动或复用开发 agentd，并传入客户端连接配置。多个终端与 Desktop 可共用默认的 `.local/dev/agentd` 数据目录和 `127.0.0.1:7547` 地址。数据目录、配置与停止方式见[开发与验证](development.md)。

## 独立运行 daemon

在仓库根目录执行：

```sh
cargo run -p agentd -- --data-dir /path/to/lyonbrown-data
```

默认监听 `127.0.0.1:7447`，本地客户端入口只允许回环地址。daemon 在数据目录中保存节点身份、`auth.token` 和 `agent.sqlite3`，并独占该目录。另一个 daemon 使用相同目录时会失败，避免多个进程同时写入节点状态。

模型请求由 daemon 发出。启动前通过进程环境或配置文件旁的 `.env` 提供 `OPENAI_API_KEY`；选择 `anthropic` 时使用 `ANTHROPIC_API_KEY`。不要将真实凭证提交到仓库。未提供模型凭证时不能执行真实模型请求。

| daemon 选项 | 环境变量 | 默认值或范围 |
| --- | --- | --- |
| `--listen` | `LYONBROWN_LISTEN` | `127.0.0.1:7447`；只允许回环地址 |
| `--data-dir` | `LYONBROWN_DATA_DIR` | 系统用户本地数据目录 |
| `--provider` | `LYONBROWN_PROVIDER` | `openai`；还支持 `openai-responses`、`openai-completions`、`anthropic` |
| `--model` | `LYONBROWN_MODEL` | `gpt-4.1`；模型是否可用取决于提供方与凭证 |
| `--api-base` | `LYONBROWN_API_BASE` | 未设置时使用对应提供方地址 |
| `--max-runs` | `LYONBROWN_MAX_RUNS` | `4`；允许 `1..=128` |
| `--run-timeout-secs` | `LYONBROWN_RUN_TIMEOUT_SECS` | `900` 秒；允许 `1..=86400` |

共享 `agent-config` 基于 `config` 与 `dotenvy` 加载配置。配置按默认值、TOML、配置文件旁的 `.env`、进程环境、显式 CLI 参数依次覆盖。默认文件为数据目录下的 `config.toml`；`--config-file` 或 `LYONBROWN_CONFIG` 可选择其他文件。dotenv 解析不会修改全局进程环境。模型端点必须使用 HTTPS，回环地址可使用 HTTP；地址不能包含凭证、查询参数或 fragment。不同 API 模式需要与实际端点匹配。

`supports_images` 默认 `false`，仅在提供方和所选模型均支持图片时显式开启 `--supports-images` 或 `LYONBROWN_SUPPORTS_IMAGES=true`。分类器、配置示例与协议对应关系见[模型与执行决策](models.md)。

所有产品进程通过共享 `agent-telemetry`，使用 `tracing`、`tracing-subscriber` 与 `tracing-appender` 写入 JSON 行日志，默认位于数据目录的 `logs`，按天轮转，每个进程前缀保留 14 个文件。`LYONBROWN_LOG_DIRECTORY` 和 `LYONBROWN_LOG_RETENTION` 可覆盖目录与保留数量；保留数量允许 `1..=365`。凭证不进入 UI 模型或配置调试输出。

## 直接运行客户端与原生服务

开发时使用 `cargo dev tui` 或 `cargo dev desktop` 自动启动并连接共享开发 daemon，参数见[开发与验证](development.md)。安装后的终端与 Desktop 在连接本机默认地址时，通过共享 launcher 检查并复用同一用户的原生服务，校验版本、协议和能力。macOS 使用 launchd，Linux 使用 systemd 用户服务；Windows 使用按当前用户 SID 区分的 SCM 服务。

服务注册锁和 `service.json` 固定保存在默认用户数据目录，不随客户端的 `--data-dir` 改变。已有服务使用其他数据目录时，launcher 会拒绝替换；修改客户端目录不能注册第二个同名服务。认证令牌仍保存在该 daemon 实际使用的数据目录中。

Windows 先安装 MSI，再从开始菜单运行 **Set up Lyonbrown service**。设置入口要求同一 Windows 账号完成 UAC 提权，并在原生凭据窗口输入服务登录密码；切换到另一个管理员账号会被拒绝。CLI 与 Desktop 共享 Program Files 中受保护的 daemon，密码不会进入命令行或配置文件。服务升级等待活动任务结束，并检查存储兼容范围与回滚条件。

直接使用 `cargo run` 连接本机默认地址时，客户端也会检查原生服务，因此要求能找到完整安装包中的 daemon，或同目录下已构建的 `agentd`。开发时优先使用上面的 `cargo dev` 命令；以下命令用于直接运行终端：

```sh
cargo run -p agent-cli -- --endpoint http://127.0.0.1:7447 --data-dir /path/to/lyonbrown-data
```

客户端通过 `--token`、`LYONBROWN_TOKEN` 或配置文件旁的 dotenv 接收 token。本机默认地址由共享 launcher 从所复用 daemon 的数据目录读取 `auth.token`；其他地址必须显式提供 token。令牌不应写入文档、Git 或聊天记录；通过环境变量或本地文件提供可避免将值写入命令历史。

| 选项 | 环境变量 | 默认值或行为 |
| --- | --- | --- |
| `--endpoint` | `LYONBROWN_ENDPOINT` | `http://127.0.0.1:7447` |
| `--token` | `LYONBROWN_TOKEN` | 本机默认地址通过 launcher 读取 `auth.token`；其他地址须提供 token |
| `--data-dir` | `LYONBROWN_DATA_DIR` | `ProjectDirs::from("org", "Lyonbrown", "Lyonbrown")` 的本地数据目录 |
| `--theme` | 无 | `auto`；还可选 `light`、`dark` |

`--data-dir` 应指向与 daemon 相同的数据目录，或通过环境变量提供相同 token。认证失败时先核对地址与令牌，不要反复提交 prompt。

连接成功后按 `Ctrl+N` 创建会话，输入消息并按 `Enter`。任务结果和审批来自 daemon。关闭客户端后可重新连接原会话查询结果。详细按键见[终端与桌面客户端](clients.md)。

配置依据：`apps/agentd/src/config.rs`、`crates/agent-config/src/load.rs`、`crates/agent-runtime/src/provider.rs` 和 `apps/agent-cli/src/config.rs`。
