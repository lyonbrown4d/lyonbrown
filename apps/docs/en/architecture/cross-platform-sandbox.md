# Cross-Platform Sandbox Design

## Goals and boundary

The sandbox is an execution layer between Tool Runtime and the operating system. It is neither a provider concern nor equivalent to user approval: approval decides whether an attempt is permitted, while the sandbox limits what an approved program can actually do. M3's `cap-std` filesystem tools already provide capability-relative path confinement on Windows, Linux, and macOS, but the current Shell still runs as a host process and must not be labeled sandboxed.

The threat model covers filesystem access outside the workspace, inherited credentials and environment, networking, child-process escape, interference with other processes, and exhaustion of CPU, memory, process count, output, or wall time. Kernel exploits, hardware side channels, and malicious code already holding administrator/root privileges are out of scope.

## Common policy

`agent-sandbox` accepts a platform-independent `SandboxPolicy`:

```rust
pub struct SandboxPolicy {
    pub filesystem: Vec<MountRule>, // workspace RO/RW; everything else hidden
    pub network: NetworkPolicy,     // Deny, Loopback, Allowlisted
    pub environment: EnvPolicy,     // empty by default; explicit keys/SecretRefs only
    pub process: ProcessPolicy,     // child spawning and process-count limits
    pub resources: ResourceLimits,  // wall time, CPU, memory, output
    pub enforcement: Enforcement,   // Strict, BestEffort, Off
}
```

- `Strict` fails the tool when no backend can satisfy the policy or a required constraint cannot be enforced.
- `BestEffort` requires an explicit interactive choice and records every unenforced constraint; it never silently degrades.
- `Off` is the current approved host shell and must be displayed as “not isolated.”

Each backend returns machine-readable `SandboxCapabilities` and `SandboxStrength`. Kernel records `SandboxSelected`, `SandboxUnavailable`, `SandboxViolation`, and resource-termination reasons for audit and resume.

## Execution backends

### WASI (preferred portable backend)

Tools compiled as WebAssembly Components run in embedded Wasmtime. Only policy-authorized directories are preopened; networking, host environment variables, and arbitrary syscalls are absent by default. Fuel/epoch controls, memory limits, and wall timeout constrain resources. This is the most consistent strong-isolation path across Windows, Linux, and macOS, but it cannot directly execute arbitrary host commands.

### Native Linux backend

Use Landlock for filesystem hierarchy restrictions and apply the network and IPC controls supported by the runtime-detected ABI. Combine it with `no_new_privs`, seccomp, a separate process group, and cgroup v2 resource limits when available. The backend reports its actual strength; an old kernel must not masquerade as full isolation in `Strict` mode.

### Native Windows backend

Use AppContainer/a low-integrity token to restrict filesystem, registry, network, and process access. A Job Object owns the full descendant tree and applies memory, CPU, process-count, and termination limits. Workspace access is granted through explicit capabilities/ACL brokering rather than inheriting all of the Agent user's authority.

### Native macOS backend

A signed app bundle uses App Sandbox entitlements and has embedded helpers inherit the sandbox. A plain CLI cannot assume an equivalent stable native interface: portable `Strict` execution prefers WASI; arbitrary native commands require an explicitly configured container/VM backend, otherwise execution is denied instead of relying on deprecated or private interfaces.

## Tool Runtime integration

```text
Model proposal
    → ApprovalPolicy
    → SandboxPolicyCompiler
    → SandboxBackend::spawn
    → capped/redacted ToolResult
    → AgentEvent audit log
```

Built-in read/write/list retain their `cap-std` implementation and do not spawn a process. Shell, MCP child processes, and future plugins all go through `SandboxExecutor`. Secrets are resolved immediately before spawn and enter only explicitly allowed child environments; sandbox events and output continue through redaction before persistence.

## Delivery and acceptance

1. M3: capability-relative FS, approval, timeout, and output limits; Shell explicitly reports `host/unsandboxed`.
2. Phase 2 S1: add `agent-sandbox`, policy compilation, capability detection, a WASI backend, and a backend conformance suite.
3. S2: native Linux and Windows backends; macOS first covers signed apps while CLI uses WASI/container execution.
4. S3: network allowlists, resource accounting, audit UI, and policy templates.

Cross-platform CI must prove that outside-workspace access fails, network and host environment are absent by default, descendants cannot escape or survive, timeout terminates the whole tree, resource limits apply, missing `Strict` backends fail closed, and every degradation is visible in both the event log and TUI/CLI.

## References

- [Wasmtime Security](https://docs.wasmtime.dev/security.html)
- [Linux Landlock userspace API](https://www.kernel.org/doc/html/latest/userspace-api/landlock.html)
- [Windows AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)
- [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
- [Apple App Sandbox](https://developer.apple.com/documentation/security/app-sandbox)
