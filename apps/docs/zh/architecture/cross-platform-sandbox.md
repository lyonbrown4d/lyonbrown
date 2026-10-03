# 跨平台 Sandbox 设计

## 目标与边界

Sandbox 是 Tool Runtime 与操作系统之间的独立执行层，不属于 provider，也不等同于用户审批。审批回答“是否允许尝试”，sandbox 限制“获准的程序实际能做什么”。M3 的 `cap-std` 文件工具已经跨 Windows、Linux 和 macOS 提供 capability-relative 路径隔离，但当前 Shell 仍是宿主进程，不能标记为 sandboxed。

威胁模型覆盖工作区外文件读写、凭据和环境变量继承、网络访问、子进程逃逸、跨进程影响，以及 CPU、内存、进程数、输出和执行时间耗尽。不承诺抵御内核漏洞、硬件侧信道或拥有管理员/root 权限的恶意代码。

## 统一策略

`agent-sandbox` 只接受与平台无关的 `SandboxPolicy`：

```rust
pub struct SandboxPolicy {
    pub filesystem: Vec<MountRule>, // workspace RO/RW；其他路径默认不可见
    pub network: NetworkPolicy,     // Deny、Loopback、Allowlisted
    pub environment: EnvPolicy,     // 默认清空，仅显式键和 SecretRef 注入
    pub process: ProcessPolicy,     // 是否可派生子进程、最大进程数
    pub resources: ResourceLimits,  // wall time、CPU、memory、output
    pub enforcement: Enforcement,   // Strict、BestEffort、Off
}
```

- `Strict`：缺少满足策略的后端或某项约束无法执行时，工具直接返回失败结果。
- `BestEffort`：只允许交互式用户明确选择；事件必须记录未生效的约束，不能静默降级。
- `Off`：等价于当前获批的 host shell，UI 必须显示“未隔离”。

后端返回机器可读的 `SandboxCapabilities` 与 `SandboxStrength`。Kernel 记录 `SandboxSelected`、`SandboxUnavailable`、`SandboxViolation` 和资源终止原因，以支持审计和 resume。

## 执行后端

### WASI（首选可移植后端）

可编译为 WebAssembly Component 的工具通过嵌入式 Wasmtime 运行。只预打开策略授权的目录，不默认提供网络、宿主环境变量或任意系统调用；fuel/epoch、memory limit 和 wall timeout 控制资源。这是 Windows、Linux、macOS 上语义最一致的强隔离路径，但不能直接运行任意宿主命令。

### Linux 原生后端

使用 Landlock 限制文件层级，并按运行时探测到的 ABI 施加可用的网络和 IPC 约束；结合 `no_new_privs`、seccomp、独立进程组以及可用时的 cgroup v2 资源限制。后端必须根据内核能力报告实际强度，旧内核在 `Strict` 下不得伪装为完整隔离。

### Windows 原生后端

使用 AppContainer/低完整性令牌限制文件、注册表、网络和进程访问，用 Job Object 管理整个子进程树并施加内存、CPU、进程数和终止限制。工作区权限通过显式 capability/ACL broker 授予，不继承 Agent 的全部用户权限。

### macOS 原生后端

签名的应用包采用 App Sandbox entitlement，并让嵌入式 helper 继承 sandbox。纯 CLI 无法假设拥有等价、稳定的原生接口：`Strict` 的可移植路径优先使用 WASI；需要执行任意原生命令时，使用明确配置的容器/VM 后端，否则拒绝，而不是依赖已弃用或私有接口。

## Tool Runtime 集成

```text
Model proposal
    → ApprovalPolicy
    → SandboxPolicyCompiler
    → SandboxBackend::spawn
    → capped/redacted ToolResult
    → AgentEvent audit log
```

内置 read/write/list 保持 `cap-std` 实现，不为简单文件操作启动子进程。Shell、MCP 子进程和未来插件统一经过 `SandboxExecutor`。Secret 仍在启动前最后一刻解析，只进入明确允许的子进程环境；sandbox 事件与输出在写入日志前继续经过 redaction。

## 交付顺序与验收

1. M3：完成 capability-relative FS、审批、超时、输出限制；Shell 明确报告 `host/unsandboxed`。
2. Phase 2 S1：建立 `agent-sandbox`、策略编译器、能力探测、WASI backend 和 backend conformance suite。
3. S2：实现 Linux 与 Windows 原生后端；macOS 先覆盖签名应用，CLI 采用 WASI/容器路径。
4. S3：网络 allowlist、资源计量、审计 UI 与策略模板。

跨平台 CI 必须验证：工作区外读写失败、默认无网络/无宿主环境、孙进程不能逃离或残留、超时会终止整个进程树、资源上限生效、`Strict` 后端缺失时 fail-closed，以及任何降级都出现在事件日志和 TUI/CLI 中。

## 参考

- [Wasmtime Security](https://docs.wasmtime.dev/security.html)
- [Linux Landlock 用户态 API](https://www.kernel.org/doc/html/latest/userspace-api/landlock.html)
- [Windows AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)
- [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
- [Apple App Sandbox](https://developer.apple.com/documentation/security/app-sandbox)
