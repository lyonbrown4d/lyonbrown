# TUI 本机真实场景黑盒验收

> 状态：部分通过，生产工具链路阻塞
> 验收日期：2026-10-06
> 环境：Windows PowerShell、本机 PTY、Node.js 24、真实 `agent` 构建产物、真实本地 daemon

## 1. 验收边界

本轮从用户可执行入口启动 `apps/agent-cli/dist/main.js`，通过真实 PTY 操作 TUI，并连接
由产品启动器发现或拉起的认证 daemon。模型下载、GGUF 加载、推理、Snapshot 更新和
会话恢复均使用真实实现，不以测试替身代替。

设计态审批和 Inspector 的复杂工具证据另有类型化渲染回归测试；只有能够穿过生产
daemon 的路径才计入黑盒通过。设计图不是本页的运行证据。

## 2. 真实场景结果

| 场景               | 结果     | 观察证据                                                                                                 |
| ------------------ | -------- | -------------------------------------------------------------------------------------------------------- |
| 启动与连接         | 通过     | TUI 先显示，随后连接认证 daemon；启动不被模型下载阻塞。                                                  |
| 模型缺失           | 通过     | 显示推荐的 `qwen3-1.7b-q4_k_m`（约 1.03 GiB），没有强制下载。                                            |
| 中英文切换         | 通过     | 模型设置阶段按 `F2` 可在简体中文与英文之间即时切换。                                                     |
| 后台下载           | 通过     | 用户按 `D` 后依次观察到 WaitingForLock、Downloading 0–100%、Verifying、Activating、Ready；界面持续响应。 |
| 真实本地推理       | 通过     | 对精确回复提示返回 `READY`，活动用量来自真实 Snapshot。                                                  |
| 运行中引导         | 通过     | 活跃 turn 中普通 Enter 发送 steering，后续输出按新指令收敛。                                             |
| 下一步排队         | 通过     | `Alt-Enter` 显示排队项，并在当前 turn 完成后成为下一 turn。                                              |
| Activity Inspector | 通过     | `Ctrl-O` 展示同步状态、活动、route、token 用量和助手证据，并可再次关闭。                                 |
| 主题控制面板       | 通过     | `Ctrl-P` 搜索进入控制面板；六主题可见；Space 预览不覆盖已保存值；Esc 恢复。                              |
| 偏好持久化         | 通过     | 保存 Slate Blue 后正常退出并重启，主题恢复；随后可保存回默认 Hearth Teal。                               |
| 会话恢复           | 通过     | 重启直接使用已缓存模型进入 Ready，并恢复已有 transcript。                                                |
| 80 列响应式        | 通过     | 模型保留在 context line；长回复完整换行且每条回复只出现一个 Agent 标签；窄控制面板动作不截断。           |
| 正常退出           | 通过     | `/quit` 后客户端退出码为 0；已接受的 daemon 下载不被隐式取消。                                           |
| 工具调用与真实审批 | **阻塞** | 模型没有收到可用工具，工具计数为 0；无法由生产链路产生审批并提交决定。                                   |
| 会话模型切换       | **阻塞** | 控制面板已实现，但生产 daemon 尚不支持 `select-session-model`。                                          |

## 3. 阻塞根因

工具/审批场景不是 TUI 渲染缺失：

- `apps/agentd/node-src/production-kernel.ts` 的生产组合当前注入空工具注册表，并对工具
  权限默认拒绝；
- `apps/agentd/node-src/local-model-adapter.ts` 明确拒绝非空 tools、tool-call 和 tool-result
  消息；
- `apps/agentd/node-src/service.ts` 尚未实现 `respond-approval` 与
  `select-session-model` 命令。

因此，本轮不会把 fixture 中可渲染的审批或工具证据写成真实黑盒通过。要关闭这两个
阻塞项，后续迭代必须先在 daemon 接入真实工具注册表、模型 tool-calling 协议、默认
拒绝的审批门和相应 gRPC 命令，再复跑同一 PTY 场景。

## 4. 自动化回归补充

自动化覆盖主界面、模型设置、命令面板、六主题控制面板、中英文状态、内联审批、
Inspector 证据、窄终端换行、steering/queue/cancel 命令边界、偏好文件校验和终端文本
安全。真实黑盒与自动化测试共同构成当前验收证据；任一层失败都不能宣称生产流程完整。
