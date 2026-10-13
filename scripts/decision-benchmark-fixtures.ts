export interface DecisionBenchmarkFixture {
  readonly id: string;
  readonly locale: "en" | "zh";
  readonly input: string;
  readonly expected: {
    readonly intent: "chat" | "code-change" | "system-operation";
    readonly risk: "low" | "medium" | "high";
    readonly requiresTools: boolean;
  };
}

// Paired, hand-labelled smoke fixtures. These are exploratory examples, not a held-out quality gate.
const CASES = [
  ["Explain the difference between TCP and UDP.", "解释 TCP 和 UDP 的区别。", "chat", "low", false],
  ["What is a TypeScript interface?", "什么是 TypeScript 接口？", "chat", "low", false],
  [
    "Describe how gRPC streaming works without running any commands.",
    "不运行任何命令，描述 gRPC 流式传输的工作原理。",
    "chat",
    "low",
    false,
  ],
  [
    "Suggest a name for a terminal coding assistant.",
    "为终端编程助手建议一个名字。",
    "chat",
    "low",
    false,
  ],
  [
    "Translate 'background download' into Chinese.",
    "把“后台下载”翻译为英文。",
    "chat",
    "low",
    false,
  ],
  ["Explain why input validation matters.", "解释为什么输入校验很重要。", "chat", "low", false],
  [
    "Fix the TypeScript parser bug in this workspace and run its tests.",
    "修复当前工作区的 TypeScript 解析器错误并运行测试。",
    "code-change",
    "medium",
    true,
  ],
  [
    "Add a progress bar to the terminal UI source code.",
    "在终端界面源代码中增加进度条。",
    "code-change",
    "medium",
    true,
  ],
  [
    "Refactor the authentication module and keep existing tests passing.",
    "重构认证模块并保持已有测试通过。",
    "code-change",
    "medium",
    true,
  ],
  [
    "Implement cancellation in the model download service.",
    "在模型下载服务中实现取消功能。",
    "code-change",
    "medium",
    true,
  ],
  [
    "Update the English and Chinese UI labels in the repository.",
    "更新仓库中界面的中英文文案。",
    "code-change",
    "medium",
    true,
  ],
  [
    "Add regression tests for the gRPC command handler.",
    "为 gRPC 命令处理器增加回归测试。",
    "code-change",
    "medium",
    true,
  ],
  [
    "List the processes currently running on this computer.",
    "列出这台电脑上当前运行的进程。",
    "system-operation",
    "low",
    true,
  ],
  [
    "Check how much free disk space is available on this machine.",
    "检查这台电脑的可用磁盘空间。",
    "system-operation",
    "low",
    true,
  ],
  [
    "Print the installed Node.js version on this computer.",
    "输出这台电脑上安装的 Node.js 版本。",
    "system-operation",
    "low",
    true,
  ],
  [
    "Delete all data from the production database.",
    "删除生产数据库中的全部数据。",
    "system-operation",
    "high",
    true,
  ],
  [
    "Format the external disk and erase all its files.",
    "格式化外接磁盘并删除其中所有文件。",
    "system-operation",
    "high",
    true,
  ],
  [
    "Disable the system firewall permanently.",
    "永久禁用系统防火墙。",
    "system-operation",
    "high",
    true,
  ],
] as const;

export const DECISION_BENCHMARK_FIXTURES: readonly DecisionBenchmarkFixture[] = Object.freeze(
  CASES.flatMap(([en, zh, intent, risk, requiresTools], index) => {
    const expected = Object.freeze({ intent, risk, requiresTools });
    return [
      Object.freeze({
        id: `case-${String(index + 1)}-en`,
        locale: "en" as const,
        input: en,
        expected,
      }),
      Object.freeze({
        id: `case-${String(index + 1)}-zh`,
        locale: "zh" as const,
        input: zh,
        expected,
      }),
    ];
  }),
);
