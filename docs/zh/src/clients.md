# 终端与桌面客户端

两个客户端均使用 `agent-client` 访问 daemon。界面中的会话、运行结果和审批来自服务端；客户端不内置另一套 Agent Loop。

## 终端工作台

```sh
cargo run -p agent-cli -- --help
cargo run -p agent-cli -- --endpoint http://127.0.0.1:7447 --data-dir /path/to/lyonbrown-data
```

工作台包含会话列表、对话、任务、审批和输入区。初次连接失败时显示 `Disconnected`，不会展示虚构的模型回复。提交请求失败时保留草稿，不自动重放请求；只有服务端确认接受后才清空对应草稿。

| 按键 | 操作 |
| --- | --- |
| `Ctrl+N` | 以当前工作目录创建会话 |
| `Tab` | 切换到下一个会话并加载运行记录 |
| `Enter` | 向已选会话提交输入 |
| `Ctrl+R` | 重新连接 daemon |
| `Ctrl+X` | 请求取消当前运行 |
| `Ctrl+Y` / `Ctrl+E` | 批准 / 拒绝首个待处理审批 |
| `PageUp` / `PageDown` | 滚动对话 |
| `Ctrl+C` / `Ctrl+D` | 退出客户端 |

审批区展示工具和参数。审批决定由 daemon 校验并保存，界面按服务端结果更新。任务记录每两秒刷新一次，新提交的运行同时订阅事件流。客户端退出时恢复终端状态，不向 daemon 发送取消请求。

`--theme auto` 根据 `COLORFGBG` 选择文字配色，并保留终端背景。无法检测时使用深色终端配色；可显式指定 `--theme light` 或 `--theme dark`。

## 原生计算机连接

CLI/TUI 与 Desktop 在自身进程内嵌入 ComputerRuntime。默认构建不启用原生 SDK；需要显式构建客户端的 `native-computer` feature，见[开发与验证](development.md#原生计算机客户端构建)。这不启用 daemon 的模型工具，也不请求操作系统权限。目标发现只列出已有权限下的窗口或显示器元数据，不截图、不发送输入；会话锁定、状态未知或缺少屏幕权限时会拒绝发现。

CLI 先列出目标，再用 daemon 中已有的非零 Session UUID 和一个目标 ID 显式挂接：

```sh
cargo run -p agent-cli --features native-computer -- computer targets
cargo run -p agent-cli --features native-computer -- computer attach --session SESSION_UUID --window WINDOW_ID --process-id PID
# 显示器范围使用 --display DISPLAY_ID，不传 --window 或 --process-id
```

`computer targets` 不连接 daemon。选择窗口时，需要同时复制目录中的窗口 ID 和 `process_id`。`computer attach` 重新发现目标，先核对 PID 完全一致，再保留描述符、建立认证连接并检查会话归属；窗口 ID 被其他进程复用时不会静默采用新目标。命令保持前台运行，按 `Ctrl+C` 断开；进程退出也撤销宿主连接。挂接失败会显示不可用原因，不自动选择其他目标。

TUI 在已连接并选定会话后，通过输入区执行本地命令：

```text
/computer targets
/computer attach window WINDOW_ID
/computer attach display DISPLAY_ID
/computer status
/computer detach
```

两个 `attach` 示例择一使用。必须先运行 `/computer targets`，再从当前目录选择；挂接使用缓存中的准确目标描述符，不按 ID 临时替换目标。Computer 区显示原绑定会话、范围和连接状态，`Alt+PageUp` / `Alt+PageDown` 滚动详情。切换会话不转移已有连接；换目标或会话前先 detach。`/computer` 前缀即使语法错误也不会提交给模型。重连或断线会撤销宿主并清空目标目录，重新挂接前必须再次发现；动作不会自动重放。

Desktop 的操作与绑定状态见下方「Computer 面板」。目标目录与权限、固定窗口身份的限制见[原生桌面执行边界](protocol.md#原生桌面执行边界)。入口接线不等于原生控制验收；macOS 无法确认会话解锁状态时仍以 `SessionStateUnknown` 拒绝，不捕获画面或输入。

## 终端任务管理

```sh
cargo run -p agent-cli -- task list --session SESSION_ID --page-size 100
cargo run -p agent-cli -- task get TASK_ID
cargo run -p agent-cli -- task reconcile TASK_ID --node-id WORKER_ID --node-endpoint https://worker.example:7448
```

`task dispatch --help` 列出派发参数。必须显式提供稳定的 Task UUID、Attempt UUID、幂等键及同一 Session 中正在运行的父 Run；daemon 校验所有权、工作区、截止时间与能力范围。`task cancel` 请求取消；`task accept` 必须使用 `--accepted true` 或 `--accepted false` 显式决定是否采纳 Worker 结果。

配对身份和凭据留在 daemon，节点地址不授予信任。提交响应丢失时，保留原 Task ID，通过 `get` 和 `reconcile` 核对，不生成新的任务身份自动重试。分页返回 `next_task_id`，后续查询通过 `--after` 继续。

## GPUI 桌面

```sh
cargo run -p agent-desktop
```

桌面使用 GPUI 和 `gpui-kit` 组件，包含会话侧栏、多聊天标签页、对话输入，以及 Browser、Computer、Files、Editor、Diff 和 Tasks 停靠面板。自定义标题栏复用组件库的 `TitleBar`；macOS 使用系统原生的关闭、最小化和缩放按钮。左侧会话栏默认展开，右侧工作台默认收起。标题栏上的左右面板按钮分别控制两侧；即使两侧均已收起，也可以从标题栏重新展开。

### 设置与连接

标题栏的 Settings 按钮打开独立设置页。Daemon 地址和访问令牌只在设置页编辑；Workspace path 用于当前标签页的新聊天。连接地址、令牌和工作区路径应与 daemon 对应；远程 daemon 的工作区路径指向远端文件系统。Appearance 提供手动选择的 Light 和 Dark 外观。选择 Back to chats 或按 `Esc` 返回聊天时，保留两侧面板的展开状态和聊天草稿。

存在活动运行、尚未结束的请求或未保存的文件修改时，Connect 不可用。保存未改动的文件时，也要等待保存请求结束后再重连。未保存修改检查同时覆盖当前 Editor 和其他会话缓存的文件草稿。连接请求尚未完成时，旧 Editor 保持只读，避免向待切换的连接写入。

### 聊天标签页与草稿

使用「＋ New chat」创建聊天标签页；从侧栏选择会话时，打开对应标签页，或切换到已打开的同一会话。每个标签页有独立标识，并保留自己的原生输入组件、工作区路径、输入草稿、消息、审批和运行状态。后台标签页继续接收自己的运行事件，响应不会写入当前可见聊天。

关闭标签页只停止该标签页的本地事件订阅，不向 daemon 发送取消请求。关闭最后一个标签页后，界面保留一个新的空白标签页。关闭桌面也不默认取消 daemon 作业。

Editor 与差异视图复用组件库，文件操作通过 daemon 的工作区协议执行。切换聊天时，Editor 按会话缓存并恢复打开文件及未保存文本；文件保存请求尚未完成时，必须等待保存结束才能切换、新建或关闭当前聊天。标签页、草稿、面板状态和外观选择均没有跨应用重启的恢复保证。

Browser 的连接、导航、关闭或接管请求尚未完成时，也必须等待结果才能切换、新建或关闭当前聊天，以及重新连接 daemon。Space 创建成功但地址打开失败时，界面保留该 Space，可修正地址后继续打开，或显式关闭。切换接管状态后，面板清除旧帧并重新获取；迟到的旧帧、空帧或错误不会覆盖新的读取状态。

### Computer 面板

先连接 daemon 并选择已有聊天，再打开 Computer 面板：

1. 选择 Refresh targets，读取本机目标目录。
2. 在组件库提供的可搜索 Select 中明确选择窗口或显示器。刷新后的选择为空，不自动选中第一项。
3. 选择 Attach to selected chat，将准确的目标描述符挂接到当前选定聊天。
4. 使用完毕后选择 Detach。

面板分别显示 Selected chat、Bound chat 和 Bound scope。切换或关闭聊天标签页不会将原连接转给其他聊天；Bound chat 始终显示原会话，挂接期间不能刷新或另选目标。目标发现、挂接或断开尚未完成时，先等待结果，再切换、新建或关闭聊天及重新连接。

Connect 开始尝试时就撤销旧宿主并清空目录，即使新连接认证失败也不保留旧授权。刷新、聊天绑定及连接变化使过期异步结果失效；迟到的挂接结果被丢弃并取消宿主。断线不会重放动作；显式 Detach 后再刷新并重新选择。断开失败会提示原生调用可能仍在结束，新的挂接可能暂不可用。关闭桌面撤销本地宿主，不默认取消 daemon 作业。

### 桌面快捷键

以下 `Cmd/Ctrl` 表示使用 `Cmd` 或 `Ctrl` 组合键。

| 按键 | 操作 |
| --- | --- |
| `Cmd/Ctrl+B` | 展开或收起左侧会话栏 |
| `Cmd/Ctrl+Alt+B` | 展开或收起右侧工作台 |
| `Cmd/Ctrl+,` | 打开 Settings |
| `Esc` | 从 Settings 返回聊天 |
| `Cmd/Ctrl+T` | 新建聊天标签页 |
| `Cmd/Ctrl+W` | 关闭当前标签页；在 Settings 中返回聊天 |
| `Cmd/Ctrl+L` | 返回聊天并聚焦输入区 |
| `Cmd/Ctrl+Enter` | 在输入区提交输入 |
| `Cmd/Ctrl+S` | 在 Editor 中保存文件 |

原生窗口是否可启动取决于目标平台的图形环境和依赖。编译通过不能替代窗口创建、输入、主题切换以及真实 daemon 连接的运行验收，也不能据此认定 Windows、macOS 和 Linux 均已验收。

源码入口：`apps/agent-cli/src/main.rs`、`apps/agent-desktop/src/main.rs`。桌面交互见 `apps/agent-desktop/src/lib.rs`、`chrome.rs`、`settings.rs`、`tabs.rs`、`tabs/` 和 `workbench/drafts.rs`。

实际窗口、明暗切换、审批与文件读取截图及其验收边界见[桌面运行截图与验收范围](runtime-evidence.md)。
