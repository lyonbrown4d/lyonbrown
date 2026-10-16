# 发布制品

> 状态：已验证可移动的 Windows x64、macOS arm64 开发制品，但它们还不是已签名的 release candidate
> 或安装器。

2026-10-16 工作区实现已扩展为 Windows、macOS、Linux 的 x64/arm64 本机打包入口；每种 OS/架构都必须在对应环境构建和验收，不提供交叉编译保证。新增平台的最终制品验收记录仍待补充。

## 构建契约

在干净的仓库根目录运行：

```text
pnpm package:release -- --output artifacts/lyonbrown-win32-x64
```

输出必须是 `artifacts/` 下尚不存在的子目录；打包命令拒绝覆盖旧路径。命令会构建
`agent-cli` 及其生产依赖闭包，使用 pnpm hoisted deploy 布局，嵌入仓库固定的当前
Node 24 runtime，并在 Windows 生成可移动的 `agent.cmd`，在 macOS/Linux 生成可执行的 `agent` shell 启动器。后者使用制品内的 `runtime/node`。TUI 通过公开的
`@lyonbrown/agentd/daemon` package entry 定位 daemon，不再依赖 monorepo 目录布局。

制品不携带 GGUF。模型检查仍是异步的，只有用户显式操作才会开始模型下载。

部署使用 pnpm 的专用锁文件生成机制，在该命令中显式开启 workspace 注入和文件复制，不修改仓库全局安装配置。文件复制避免后续源码构建通过硬链接修改已生成的制品。不使用会重新解析依赖的 `--legacy` 路径；否则制品可能偏离仓库锁定的 Pi 依赖组合。实际制品 smoke 必须验证 daemon 能独立导入。

在 macOS/Linux 上可省略 `--output` 使用本机默认目录，或指定 `artifacts/lyonbrown-darwin-arm64` 等对应路径。打包由 moon 构建依赖图；移动制品目录后，启动器仍应保留带空格、Unicode 和 shell 元字符的原始参数。

## 制品 smoke

打包会自动执行以下检查；制品复制或改名后也可以重复执行：

```text
pnpm smoke:release-artifact -- --artifact artifacts/lyonbrown-win32-x64
```

smoke 使用制品内嵌 runtime 验证：

- package manifest 与 CLI 版本一致；
- Windows 制品不包含 symlink 或 junction；POSIX 制品只允许目标为制品内部普通文件的相对链接，拒绝绝对链接、目录链接和越界链接，依赖解析不绑定构建路径；
- 打包后的 daemon entry 可以脱离源码 workspace 独立导入；
- 打包后的 `better-sqlite3` 可以打开内存数据库；
- 打包后的 `node-llama-cpp` 在禁止源码构建与下载时可以加载平台预编译二进制：macOS 自动选择原生后端，Windows/Linux 验证 CPU 后端。

2026-10-06，Windows x64 开发制品已通过 smoke，并在目录改名后再次通过。该制品包含
20,263 个文件，共 943,666,133 bytes。

2026-10-16，macOS arm64 开发制品通过锁定依赖部署、内嵌 Node、独立 daemon 导入、SQLite 和原生预编译包 smoke；在含空格和中文的目录中再次通过，并验证启动器版本。部署后的文件已解除与源码的硬链接。该验收没有下载或加载 GGUF，不能代替真实模型任务验收。

## 剩余发布工作

当前开发制品刻意保留完整生产依赖闭包。`node-llama-cpp` 会带入多套可选 CPU、Vulkan
与 CUDA package，因此尚未做体积优化。发布设计需要决策：拆分 CPU、Vulkan、CUDA
制品，还是保留更大的组合制品；任何裁剪都必须证明不会启用运行时编译或下载 fallback。

压缩归档、checksum/SBOM、代码签名、安装器、干净机器验证、升级/回滚/卸载、Windows
arm64 验证，以及基于最终制品重复固定真实 GGUF 全流程，仍是正式发布前的必做项。
