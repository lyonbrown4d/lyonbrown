import { describe, expect, it, vi } from "vitest";

import { createOpenJevWorker } from "@lyonbrown/decision-engine/internal/open-jev-worker";
import type { OpenJevRuntime } from "@lyonbrown/decision-engine/internal/open-jev-runtime";
import type { OpenJevWorkerTask } from "@lyonbrown/decision-engine/internal/worker-engine";
import { injectedOpenJevConfiguration } from "@lyonbrown/decision-engine/test/model-environment";

const task = (): OpenJevWorkerTask => ({
  configuration: injectedOpenJevConfiguration(),
  state: "Fix the failing build.",
  questions: [
    {
      type: "choice",
      instructions: "Which route should handle this task?",
      options: ["chat", "agent"],
    },
  ],
});

describe("OpenJev Piscina worker", () => {
  it("disables remote access, loads the local model once, and reuses it", async () => {
    const answers = [{ type: "choice", choice: "agent" }];
    const runtime: OpenJevRuntime = {
      decide: vi.fn().mockResolvedValue(answers),
      dispose: vi.fn().mockResolvedValue(undefined),
    };
    const load = vi.fn().mockResolvedValue(runtime);
    const disableRemoteModels = vi.fn();
    const run = createOpenJevWorker({ load, disableRemoteModels });

    await expect(run(task())).resolves.toEqual(answers);
    await expect(run(task())).resolves.toEqual(answers);

    expect(disableRemoteModels).toHaveBeenCalledOnce();
    expect(load).toHaveBeenCalledOnce();
    expect(load).toHaveBeenCalledWith({
      model: injectedOpenJevConfiguration().modelPath,
      device: "cpu",
      dtype: "q4",
      truncation: "error",
    });
    expect(runtime.decide).toHaveBeenCalledTimes(2);
  });

  it("rejects attempts to change the model configuration inside one worker", async () => {
    const runtime: OpenJevRuntime = {
      decide: vi.fn().mockResolvedValue([]),
      dispose: vi.fn().mockResolvedValue(undefined),
    };
    const run = createOpenJevWorker({
      load: vi.fn().mockResolvedValue(runtime),
      disableRemoteModels: vi.fn(),
    });

    await run(task());
    await expect(
      run({
        ...task(),
        configuration: { ...task().configuration, modelId: "different-model" },
      }),
    ).rejects.toThrow("OPEN_JEV_WORKER_CONFIGURATION_CHANGED");
  });
});
