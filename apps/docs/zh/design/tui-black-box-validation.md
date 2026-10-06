# TUI 本机真实场景黑盒验收

> 状态：核心 TUI 通过；生产工具、审批与模型选择阻塞已关闭
> 验收日期：2026-10-07
> 环境：Windows PowerShell、本机 PTY、Node.js 24、真实 `agent` 构建产物、真实本地 daemon

## 1. 验收边界

本轮从用户可执行入口启动 `apps/agent-cli/dist/main.js`，通过真实 PTY 操作 TUI，并连接
由产品启动器发现或拉起的认证 daemon。模型下载、GGUF 加载、推理、Snapshot 更新和
会话恢复均使用真实实现，不以测试替身代替。

设计态审批和 Inspector 的复杂工具证据另有类型化渲染回归测试；只有能够穿过生产
daemon 的路径才计入黑盒通过。设计图不是本页的运行证据。

## 2. 真实场景结果

| 场景               | 结果     | 观察证据                                                                                                           |
| ------------------ | -------- | ------------------------------------------------------------------------------------------------------------------ |
| 启动与连接         | 通过     | TUI 先显示，随后连接认证 daemon；启动不被模型下载阻塞。                                                            |
| 模型缺失           | 通过     | 显示 daemon 推荐模型且没有强制下载；首次自动推荐受 1 GiB 下载预算约束。                                            |
| 中英文切换         | 通过     | 模型设置阶段按 `F2` 可在简体中文与英文之间即时切换。                                                               |
| 后台下载           | 通过     | 用户按 `D` 后依次观察到 WaitingForLock、Downloading 0–100%、Verifying、Activating、Ready；界面持续响应。           |
| 真实本地推理       | 通过     | 对精确回复提示返回 `READY`，活动用量来自真实 Snapshot。                                                            |
| 运行中引导         | 通过     | 活跃 turn 中普通 Enter 发送 steering，后续输出按新指令收敛。                                                       |
| 下一步排队         | 通过     | `Alt-Enter` 显示排队项，并在当前 turn 完成后成为下一 turn。                                                        |
| Activity Inspector | 通过     | `Ctrl-O` 展示同步状态、活动、route、token 用量和助手证据，并可再次关闭。                                           |
| 主题控制面板       | 通过     | `Ctrl-P` 搜索进入控制面板；六主题可见；Space 预览不覆盖已保存值；Esc 恢复。                                        |
| 偏好持久化         | 通过     | 保存 Slate Blue 后正常退出并重启，主题恢复；随后可保存回默认 Hearth Teal。                                         |
| 会话恢复           | 通过     | 重启直接使用已缓存模型进入 Ready，并恢复已有 transcript。                                                          |
| 80 列响应式        | 通过     | 模型保留在 context line；长回复完整换行且每条回复只出现一个 Agent 标签；窄控制面板动作不截断。                     |
| 正常退出           | 通过     | `/quit` 后客户端退出码为 0；已接受的 daemon 下载不被隐式取消。                                                     |
| 工具调用与真实审批 | 分层通过 | 真实 GGUF 产生 `write` 调用，daemon 投影待审批，client `allow-once` 后文件准确落盘；TUI 审批界面有键盘与渲染回归。 |
| 会话模型切换       | 分层通过 | 真实 client 的 pinned/auto 选择均由 daemon 接受并持久投影；TUI 控制面板只发送 daemon 公布的 route。                |

## 3. 已关闭的生产阻塞项

2026-10-07 的生产 composition 真实 GGUF smoke 已关闭原阻塞项：

- production Kernel 注入 Pi Coding Agent 的 `read`、`write`、`edit` 工作区工具；
  `@openclaw/fs-safe` 以 native-required 模式提供根目录能力、身份校验 I/O、原子写入、
  1 MiB 文件上限，以及链接/竞态下的 fail-closed 行为；
- 本地模型适配器支持类型化 tool definition、assistant tool-call 与 tool-result history；
- 完整审批决策、模型选择和工具结果先写入 Lyonbrown 领域事件，再投影到 gRPC
  Snapshot；会话审批策略可从事件重放，不再只存在于内存。

真实 smoke 覆盖显式下载、取消/重试、SHA 校验、激活、模型选择、短回答、`write`
审批与落盘、长 turn 取消、缓存重启和有界关闭，并以 `REAL_GGUF_SMOKE_OK` 结束。当前
证据由既有真实 PTY 交互验收、TUI 自动化和 production C/S 黑盒共同组成；没有把设计图
或纯 fixture 单独冒充端到端证据。

## 4. 自动化回归补充

自动化覆盖主界面、模型设置、命令面板、六主题控制面板、中英文状态、内联审批、
Inspector 证据、窄终端换行、steering/queue/cancel 命令边界、偏好文件校验和终端文本
安全。真实黑盒与自动化测试共同构成当前验收证据；任一层失败都不能宣称生产流程完整。
