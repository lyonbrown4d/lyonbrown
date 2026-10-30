# 终端与桌面客户端

两个客户端均使用 `agent-client` 访问 daemon。界面中的会话、运行结果和审批来自服务端；客户端不内置另一套 Agent Loop。

## macOS 安装入口

PKG 将应用文件放在 `/Library/Application Support/Lyonbrown/Clients/{cli|desktop}/{version}-{完整目录哈希}/`，每份内容使用独立的组件收据。资源或 Helper 改变时，即使主程序和产品版本相同，也会选择不同的目录与组件标识。`/usr/local/bin/agent` 和 `/Applications/Lyonbrown.app` 分别由独立组件管理，仅作为指向所选内容的符号链接。打包器校验复制后的目录，禁止 Installer 重定位各级 bundle，不清理旧内容组件或收据。

共享 launcher 和 CEF 宿主使用 macOS 内核报告的已加载主程序路径，从对应应用目录查找随附的 agentd、framework 和 Helper。公共入口切换版本后，旧进程仍使用原版本的配套文件。Helper 是包内复制的可执行文件；沙箱入口在调用 cef-rs 前，按该加载器使用的同一路径校验沙箱库。包内容校验与操作系统安装、升级、回滚验收分别进行，详见[本地分发说明](https://github.com/lyonbrown4d/lyonbrown/blob/main/packaging/README.md)。

## 终端工作台

```sh
cargo run -p agent-cli -- --help
cargo run -p agent-cli -- --endpoint http://127.0.0.1:7447 --data-dir /path/to/lyonbrown-data
```

工作台默认显示对话和输入区。会话、任务、计算机与帮助通过上下文面板按需打开，不常驻空面板；待处理审批出现时，在输入区上方显示工具、参数和批准／拒绝操作。宽终端将上下文面板放在对话右侧，窄终端用面板替换对话区域；`Esc` 关闭面板。初次连接失败时显示 `Disconnected`，不会展示虚构的模型回复。提交请求失败时保留草稿，不自动重放请求；只有服务端确认接受后才清空对应草稿。

| 按键 | 操作 |
| --- | --- |
| `F1` / `F2` / `F3` / `F4` / `F5` | 切换帮助／会话／任务／计算机／完整审批上下文面板 |
| `Esc` | 关闭上下文面板 |
| `Ctrl+N` | 以当前工作目录创建会话 |
| `Tab` | 切换到下一个会话并加载运行记录 |
| `Enter` | 向已选会话提交输入 |
| `Shift+Enter` / `Alt+Enter` | 在输入区换行 |
| 方向键、`Home` / `End`、`Delete` | 移动光标或编辑光标处文本 |
| `Ctrl+R` | 重新连接 daemon |
| `Ctrl+X` | 请求取消当前运行 |
| `Ctrl+Y` / `Ctrl+E` | 批准 / 拒绝首个待处理审批 |
| `PageUp` / `PageDown` | 滚动已打开的上下文面板，否则滚动对话 |
| `Ctrl+C` / `Ctrl+D` | 退出客户端 |

输入区复用 `ratatui-textarea` 0.9.3，支持光标编辑、多行文本、换行显示和粘贴。应用快捷键优先于编辑器的同名映射，例如 `Ctrl+X` 仍请求取消运行，`Ctrl+Y` 仍处理审批。提交等待期间不重复发送；失败保留草稿，服务端接受后清空对应草稿及其撤销历史。

审批区展示工具和参数；紧凑布局只保留待处理数量和审批快捷键，完整参数通过 `F5` 查看。审批决定由 daemon 校验并保存，界面按服务端结果更新。对话的 Run 记录每两秒刷新一次，新提交的运行同时订阅事件流。持久 Task 按面板查询与本地管理命令刷新。客户端退出时恢复终端状态，不向 daemon 发送取消请求。

界面使用暖深色或暖浅色的纯色终端背景、正文和强调色。`--theme auto` 根据 `COLORFGBG` 选择配色；无法检测时使用深色，可显式指定 `--theme light` 或 `--theme dark`。模型回复的 Markdown 复用固定版本 `tui-markdown` 0.3.10，经公开 `StyleSheet` 接口适配主题；关闭该库的默认 features，不启用代码语法高亮依赖。

TUI 按 MVVM 分离呈现：`view_model.rs` 从客户端 `App` 状态生成只读 `ViewModel`，整理连接标签、消息、审批和当前上下文；`view.rs` 只通过 Ratatui 渲染该数据与输入组件。输入动作和后台更新分别留在 `input.rs`、`backend.rs` 与 `update.rs`，呈现层不派发 daemon 操作。

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

Desktop 的操作与绑定状态见下方「Computer 面板」。目标目录与权限、固定窗口身份的限制见[原生桌面执行边界](protocol.md#原生桌面执行边界)。入口接线不等于原生控制验收；macOS 按会话身份与锁字段策略检查，状态未知时以 `SessionStateUnknown` 拒绝。后端一旦观察到锁屏或会话失效，解锁后仍须显式重新挂接。

## 终端任务管理

```sh
cargo run -p agent-cli -- task list --session SESSION_ID --page-size 100
cargo run -p agent-cli -- task get TASK_ID
cargo run -p agent-cli -- task reconcile TASK_ID --node-id WORKER_ID --node-endpoint https://worker.example:7448
```

`task dispatch --help` 列出派发参数。必须显式提供稳定的 Task UUID、Attempt UUID、幂等键及同一 Session 中正在运行的父 Run；daemon 校验所有权、工作区、截止时间与能力范围。`task cancel` 请求取消；`task accept` 必须使用 `--accepted true` 或 `--accepted false` 显式决定是否采纳 Worker 结果。

配对身份和凭据留在 daemon，节点地址不授予信任。提交响应丢失时，保留原 Task ID，通过 `get` 和 `reconcile` 核对，不生成新的任务身份自动重试。分页返回 `next_task_id`，后续查询通过 `--after` 继续。

TUI 的 `F3` 面板查询当前会话的持久 Task，分别显示 Task 与 Worker Attempt 状态。方向键选择任务，输入区为空时按 `Enter` 查看详情，`PageDown` 查询下一页；重新打开面板回到第一页。输入区中的 `/task ...` 使用与独立 CLI 相同的子命令和参数，例如 `/task get TASK_ID`、`/task cancel TASK_ID --node-id WORKER_ID --node-endpoint https://worker.example:7448`。这些命令在本地解析，语法错误也不会成为模型 Prompt。

TUI 保存各会话草稿、未绑定草稿、所选会话、上下文面板和滚动位置。会话切换保留各自的输入；提交确认只清除原会话中仍与已提交文本一致的草稿。`/draft list` 列出本地草稿，`/draft recover SESSION_ID` 将选定草稿放入输入区，`/draft recover unbound` 读取未绑定草稿，供用户检查后决定是否提交。原会话当前不可用的草稿仍保留，不会自动附到其他会话。

## GPUI 桌面

```sh
cargo run -p agent-desktop
```

桌面使用 GPUI 和 `gpui-kit` 组件，包含按工作区分组的会话侧栏、多聊天标签页、对话输入，以及 Browser、Computer、Files、Editor、Diff 和 Tasks 停靠面板。自定义标题栏复用组件库的 `TitleBar`；macOS 使用系统原生的关闭、最小化和缩放按钮。左侧会话栏默认展开，右侧工作台默认收起。标题栏上的左右面板按钮分别控制两侧；即使两侧均已收起，也可以从标题栏重新展开；左侧收起后保留紧凑图标栏。助手回复以扁平正文显示，输入区与工作区信息分层放置，明暗外观共用 `theme.rs` 的颜色和尺寸定义。

桌面按 MVVM 组织呈现状态：`DesktopViewModel` 保存当前会话选择、运行、事件游标、消息、审批和状态信息，并按服务端的 `Session.workspace` 分组会话。它不保存网络客户端、异步运行时或连接凭据。`operations.rs` 通过客户端端口发起请求，`projection.rs` 将回复投影到状态；GPUI 视图读取状态并复用 `gpui-kit` 的按钮、输入、Markdown、停靠和调整尺寸组件。

后台结果通过 `async-channel` 2.5.0 的有界队列投递，队列满时生产者异步等待；GPUI 在主线程应用结果，并丢弃过期会话或代次的回复。Browser 帧刷新保留独立的 160 毫秒定时器，Computer 宿主存活检查保留独立的 80 毫秒定时器。窗口关闭时清理本地订阅与宿主，daemon 作业保留自身生命周期。

工作台标签栏的省略号菜单列出 Browser、Files、Editor、Diff、Tasks 和 Computer 六个面板，可直接选择；标签被窄窗口挤出时也可从这里切换。当前组件库的辅助功能树仍不能正确提供这些标签的名称，菜单入口不代表标签的辅助功能问题已解决。

Tasks 面板读取 daemon 的持久 Task，提供首页、下一页、详情、核对、取消和显式采纳／拒绝。派发前须选定有活动 Run 的会话，并填写已配对 Worker 的节点 ID、HTTPS 地址和任务说明；默认只授予只读工作区范围，选择读写范围时写入仍须经过审批，daemon 还会检查配对权限。桌面在发送前生成 Task UUID、Attempt UUID 和幂等键，截止时间为一小时。未知结果保留原身份供查询、核对，不自动重发；Worker Attempt 成功仍显示为等待 Master 采纳。

### 设置与连接

标题栏的 Settings 按钮打开独立设置页。Daemon 地址和访问令牌只在设置页编辑；Workspace path 用于当前标签页的新聊天。连接地址、令牌和工作区路径应与 daemon 对应；远程 daemon 的工作区路径指向远端文件系统。Appearance 提供手动选择的 Light 和 Dark 外观。选择 Back to chats 或按 `Esc` 返回聊天时，保留两侧面板的展开状态和聊天草稿。

存在活动运行、尚未结束的请求或未保存的文件修改时，Connect 不可用。保存未改动的文件时，也要等待保存请求结束后再重连。未保存修改检查同时覆盖当前 Editor 和其他会话缓存的文件草稿。连接请求尚未完成时，旧 Editor 保持只读，避免向待切换的连接写入。

### 聊天标签页与草稿

使用侧栏「新对话」或标签栏「＋」创建聊天标签页；从侧栏选择会话时，打开对应标签页，或切换到已打开的同一会话。每个标签页有独立标识，并保留自己的原生输入组件、工作区路径、输入草稿、消息、审批和运行状态。非活动标签页由独立的 `ChatState` 保留这些状态；后台更新按标签页标识投递，在应用结果时暂时切换对应状态，结束后恢复当前可见聊天。后台响应不会写入其他聊天。

关闭标签页只停止该标签页的本地事件订阅，不向 daemon 发送取消请求。关闭最后一个标签页后，界面保留一个新的空白标签页。关闭桌面也不默认取消 daemon 作业。

Editor 与差异视图复用组件库，文件操作通过 daemon 的工作区协议执行。切换聊天时，Editor 按会话缓存并恢复打开文件及未保存文本；文件保存请求尚未完成时，必须等待保存结束才能切换、新建或关闭当前聊天。

认证后，桌面恢复标签页顺序、活动标签页、各标签页的聊天草稿和工作区路径，以及两侧面板、停靠布局、宽度和明暗外观。恢复期间暂停编辑和导航；后台合并快照并写入本地，正常关闭窗口前等待最后一次刷盘。原会话当前不可用的标签页标为未绑定草稿，须点击「为此草稿新建会话」完成绑定后才能提交。运行、事件和审批重新向 daemon 查询，不从快照恢复执行状态。

未保存的 Editor 草稿包含原会话、文件路径、原始内容和版本。恢复后重新读取当前文件版本；文件已改变时保留草稿并报告冲突，保存仍由 daemon 校验原始版本，不覆盖较新的文件。找不到原会话的文件草稿保留为未绑定草稿，须显式绑定；恢复本身不会保存文件。

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
| `Enter` / `Cmd/Ctrl+Enter` | 在输入区提交已确认的输入；输入法组合态不发送 |
| `Shift+Enter` | 在输入区换行 |
| `Cmd/Ctrl+S` | 在 Editor 中保存文件 |

原生窗口是否可启动取决于目标平台的图形环境和依赖。编译通过不能替代窗口创建、输入、主题切换以及真实 daemon 连接的运行验收，也不能据此认定 Windows、macOS 和 Linux 均已验收。

## 本地恢复文件

客户端的恢复文件位于配置数据目录的 `client-state/` 下。`agent-client::local_state` 按客户端类型、规范化的 daemon 地址和认证后的节点身份隔离文件；同一身份的恢复文件由独占锁保护，第二个客户端实例不能同时覆盖它。写入使用同目录临时文件并原子替换，文件大小上限为 4 MiB；Unix 上目录和文件权限分别为 `0700`、`0600`。

恢复文件只保存客户端展示与输入状态。令牌、审批决定、原生执行句柄、Browser/Computer 控制权和 daemon 的执行状态均不从恢复文件取得。重启后仍须认证并向 daemon 查询；恢复草稿不会提交请求。损坏、身份不符或版本不支持的文件会保留并报告错误，不会用空白状态覆盖。

源码入口：`apps/agent-cli/src/main.rs`、`apps/agent-desktop/src/main.rs`。终端呈现见 `apps/agent-cli/src/view_model.rs`、`view.rs` 和 `markdown.rs`；桌面交互见 `apps/agent-desktop/src/view_model.rs`、`operations.rs`、`projection.rs`、`chrome.rs`、`settings.rs`、`tabs.rs`、`tabs/` 和 `workbench/drafts.rs`。

实际窗口、明暗切换、审批与文件读取截图及其验收边界见[桌面运行截图与验收范围](runtime-evidence.md)。
