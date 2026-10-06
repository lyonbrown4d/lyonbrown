import { stripTerminalSequences } from "@earendil-works/pi-tui";
import { describe, expect, it } from "vitest";
import { createInitialUiModelState } from "@lyonbrown/ui-model";

import { renderFrame } from "@lyonbrown/agent-cli/internal/surface";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

describe("accepted TUI design alignment", () => {
  it("renders the session context, queued follow-up and complete inline approval preview", () => {
    const rendered = stripTerminalSequences(
      renderFrame(
        {
          ui: createInitialUiModelState(),
          snapshot: runtimeSnapshot({
            phase: "awaiting-approval",
            sessionId: "dev-001",
            queuedFollowUps: ["Record the local build prerequisites"],
            route: {
              selected: { provider: "local", model: "qwen3-4b" },
              reason: "local preferred",
              fallbacks: [],
            },
            pendingApproval: {
              requestId: "approval-1",
              toolName: "shell",
              argumentsJson: '{"command":"Remove-Item -Recurse target\\\\stale-cache"}',
              stepId: "step-1",
              preview: {
                tool: "shell",
                action: "Remove-Item -Recurse target\\stale-cache",
                target: "target\\stale-cache",
                impact: "Deletes 126 MiB of expired build cache",
                risk: "write",
                sandbox: "host-unsandboxed",
                workingDirectory: "F:\\Projects\\lyonbrown",
                requiredSecrets: [],
              },
              deadlineRemainingMs: 30_000n,
              state: "pending",
              approvedBy: null,
            },
          }),
          overlay: null,
          notice: null,
        },
        120,
      ).join("\n"),
    );

    expect(rendered).toContain("dev-001");
    expect(rendered).toContain("local/qwen3-4b");
    expect(rendered).toContain("Record the local build prerequisites");
    expect(rendered).toContain("Remove-Item -Recurse target\\stale-cache");
    expect(rendered).toContain("HOST / unsandboxed");
    expect(rendered).toContain("Deny once");
    expect(rendered).toContain("Allow once");
    expect(rendered).toContain("Allow shell for session");
    expect(rendered).toContain("Deny remaining risky actions");
  });

  it("explains the running-turn steering and follow-up semantics next to the composer", () => {
    const rendered = stripTerminalSequences(
      renderFrame(
        {
          ui: createInitialUiModelState(),
          snapshot: runtimeSnapshot({ phase: "running-tool" }),
          overlay: null,
          notice: null,
        },
        120,
      ).join("\n"),
    );

    expect(rendered).toContain("Enter steer now · Alt-Enter queue for next step");
  });

  it("preserves the active model at 80 columns and does not mislabel model setup Enter", () => {
    const base = runtimeSnapshot({
      route: {
        selected: { provider: "local", model: "qwen3-1.7b-q4_k_m" },
        reason: "local preferred",
        fallbacks: [],
      },
      phase: "model-setup-required",
      modelSetup: {
        source: "managed-preset",
        phase: "required",
        managedModel: {
          id: "qwen3-1.7b-q4_k_m",
          displayName: null,
          recommendationReason: null,
          expectedDownloadBytes: 1_106_000_000n,
          maxBytes: 1_200_000_000n,
          minAvailableMemoryBytes: 1n,
          sha256: "a".repeat(64),
        },
        operationId: null,
        downloadedBytes: 0n,
        totalBytes: 1_106_000_000n,
        failure: null,
        allowedActions: ["download"],
      },
    });
    const rendered = stripTerminalSequences(
      renderFrame(
        {
          ui: createInitialUiModelState(),
          snapshot: base,
          overlay: null,
          notice: null,
        },
        80,
      ).join("\n"),
    );

    expect(rendered.split("\n")[0]).toContain("local/qwen3-1.7b-q4_k_m");
    expect(rendered).not.toContain("Enter send");
    expect(rendered).toContain("Local model  qwen3-1.7b-q4_k_m");
  });

  it("renders streamed multiline answers once and wraps without losing content", () => {
    const rendered = stripTerminalSequences(
      renderFrame(
        {
          ui: createInitialUiModelState(),
          snapshot: runtimeSnapshot({
            turns: [
              {
                turnId: "turn-1",
                userText: "Explain the result without truncating this longer request.",
                assistantText:
                  "\n\nFirst paragraph contains enough words to wrap naturally.\n\nSecond paragraph ends with PRESERVED_SENTINEL",
                status: "completed",
                steering: [],
                activities: [],
              },
            ],
          }),
          overlay: null,
          notice: null,
        },
        40,
      ).join("\n"),
    );

    expect(rendered.match(/Agent:/gu)).toHaveLength(1);
    expect(rendered).not.toContain("Agent:\nAgent:");
    expect(rendered).toContain("PRESERVED_SENTINEL");
    expect(rendered).toContain("truncating this longer request");
  });
});
