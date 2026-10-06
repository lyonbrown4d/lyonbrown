# 发布制品

> 状态：已实现并验证可移动的 Windows 开发制品，但它还不是已签名的 release candidate
> 或安装器。

## 构建契约

在干净的仓库根目录运行：

```text
pnpm package:release -- --output artifacts/lyonbrown-win32-x64
```

输出必须是 `artifacts/` 下尚不存在的子目录；打包命令拒绝覆盖旧路径。命令会构建
`agent-cli` 及其生产依赖闭包，使用 pnpm hoisted deploy 布局，嵌入仓库固定的当前
Node 24 runtime，并生成可移动的 `agent.cmd`。TUI 通过公开的
`@lyonbrown/agentd/daemon` package entry 定位 daemon，不再依赖 monorepo 目录布局。

制品不携带 GGUF。模型检查仍是异步的，只有用户显式操作才会开始模型下载。

## 制品 smoke

打包会自动执行以下检查；制品复制或改名后也可以重复执行：

```text
pnpm smoke:release-artifact -- --artifact artifacts/lyonbrown-win32-x64
```

smoke 使用制品内嵌 runtime 验证：

- package manifest 与 CLI 版本一致；
- 完整制品不包含 symlink 或 Windows junction，依赖解析不绑定构建路径；
- 打包后的 daemon entry 可以脱离源码 workspace 独立导入；
- 打包后的 `better-sqlite3` 可以打开内存数据库；
- 打包后的 `node-llama-cpp` 在禁止源码构建与下载时可以加载 CPU 预编译二进制。

2026-10-06，Windows x64 开发制品已通过 smoke，并在目录改名后再次通过。该制品包含
20,263 个文件，共 943,666,133 bytes。

## 剩余发布工作

当前开发制品刻意保留完整生产依赖闭包。`node-llama-cpp` 会带入多套可选 CPU、Vulkan
与 CUDA package，因此尚未做体积优化。发布设计需要决策：拆分 CPU、Vulkan、CUDA
制品，还是保留更大的组合制品；任何裁剪都必须证明不会启用运行时编译或下载 fallback。

压缩归档、checksum/SBOM、代码签名、安装器、干净机器验证、升级/回滚/卸载、Windows
arm64 验证，以及基于最终制品重复固定真实 GGUF 全流程，仍是正式发布前的必做项。
