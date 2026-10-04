# `agentd` 组合根

`apps/agentd` 是唯一生产组合根。依赖注入只存在于进程边界，不让领域对象从容器
主动解析服务。

## 所有权

daemon 构造并持有：

- 带作用域的 run/data 目录及原生安全能力；
- 生命周期锁、发现描述符、endpoint 与认证 token；
- EventStore、durable Runtime、Pi kernel session、工具、MCP 与 Secret；
- 托管 `node-llama-cpp` 模型存储和生命周期；
- 认证 gRPC 服务与 Snapshot stream。

TUI 只能得到窄化后的 `agent-launch`、`agent-client` 与 `ui-model` 接口，不能得到
数据库句柄、模型路径、原始 bearer token、供应商 Secret 或 Pi 对象。

## 启动顺序

1. 解析并校验类型化进程配置。
2. 使用稳定的文件系统身份与有效配置指纹推导 workspace scope。
3. 打开受保护的 run/data 目录并取得 daemon 生命周期 lease。
4. 打开并恢复 EventStore，再构造 Runtime 与 Pi 适配器。
5. 在私有 endpoint 启动认证 gRPC server。
6. 最后发布发现描述符；发布时服务必须已可用。

描述符发布前发生的失败不会被 client 发现，并按所有权逆序清理。进程绝不回退到
embedded Runtime。

## 关闭顺序

daemon 先停止接受 RPC 与 stream，在期限内优雅关闭 gRPC，超时后强制关闭；随后
dispose Runtime 和模型资源、关闭 EventStore、释放 lease 与目录锚点，最后移除发现
状态。每一步都必须尝试；诊断只返回脱敏后的第一个失败，不泄漏凭据或本地敏感路径。

## 测试接缝

生产组合显式注入原生能力；平台实现不可用时 fail closed。测试在同一边界使用 port
与 fake，生产默认值不得选择测试 helper。
