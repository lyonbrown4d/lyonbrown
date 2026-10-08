import { stripVTControlCharacters } from "node:util";

export interface TuiRenderEvidence {
  readonly ansiRendered: boolean;
  readonly chromeRendered: boolean;
  readonly grpcReadyRendered: boolean;
  readonly modelSetupRendered: boolean;
}

const ANSI_ESCAPE = String.fromCodePoint(0x1b);

export class TerminalTranscript {
  #raw = "";
  #capturedBytes = 0;
  #overflowed = false;

  public constructor(private readonly maxBytes: number) {
    if (!Number.isSafeInteger(maxBytes) || maxBytes < 1) {
      throw new RangeError("terminal capture limit must be a positive integer");
    }
  }

  public append(data: string): void {
    if (this.#overflowed || data.length === 0) return;
    const bytes = Buffer.from(data, "utf8");
    const remaining = this.maxBytes - this.#capturedBytes;
    if (bytes.length <= remaining) {
      this.#raw += data;
      this.#capturedBytes += bytes.length;
      return;
    }
    this.#raw += bytes.subarray(0, Math.max(0, remaining)).toString("utf8");
    this.#capturedBytes = this.maxBytes;
    this.#overflowed = true;
  }

  public get raw(): string {
    return this.#raw;
  }

  public get plain(): string {
    return stripVTControlCharacters(this.#raw);
  }

  public get overflowed(): boolean {
    return this.#overflowed;
  }
}

export function inspectTuiRender(transcript: TerminalTranscript): TuiRenderEvidence {
  const plain = transcript.plain;
  const englishModelSetup =
    plain.includes("Local model") &&
    ["Installed model", "Recommended for this hardware", "Conservative compatibility choice"].some(
      (label) => plain.includes(label),
    );
  const chineseModelSetup =
    plain.includes("本地模型") &&
    ["已安装模型", "根据当前硬件推荐", "保守兼容选择"].some((label) => plain.includes(label));
  return Object.freeze({
    ansiRendered: transcript.raw.includes(ANSI_ESCAPE),
    chromeRendered: plain.includes("LYONBROWN"),
    grpcReadyRendered: plain.includes("Ready") || plain.includes("已就绪"),
    modelSetupRendered: englishModelSetup || chineseModelSetup,
  });
}

export function requireTuiRender(transcript: TerminalTranscript): TuiRenderEvidence {
  if (transcript.overflowed) throw new Error("terminal capture limit exceeded");
  const evidence = inspectTuiRender(transcript);
  const missing: string[] = [];
  if (!evidence.ansiRendered) missing.push("ANSI terminal rendering");
  if (!evidence.chromeRendered) missing.push("TUI chrome");
  if (!evidence.grpcReadyRendered) missing.push("gRPC ready state");
  if (!evidence.modelSetupRendered) missing.push("daemon-projected model setup");
  if (missing.length > 0) throw new Error(`missing TUI evidence: ${missing.join(", ")}`);
  return evidence;
}
