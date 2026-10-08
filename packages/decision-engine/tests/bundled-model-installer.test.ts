import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import {
  BUNDLED_DECISION_MODEL,
  bundledDecisionModelFileUrl,
} from "@lyonbrown/decision-engine/internal/bundled-model-catalog";
import {
  inspectDecisionModelBundle,
  installDecisionModelBundle,
} from "@lyonbrown/decision-engine/internal/bundled-model-installer";
import type {
  DecisionModelBundleIdentity,
  DecisionModelBundleInstallLock,
  DecisionModelBundleStoragePort,
  DecisionModelStagingBundle,
} from "@lyonbrown/decision-engine/internal/model-bundle-storage";
import { afterEach, describe, expect, it, vi } from "vitest";

const roots: string[] = [];
const TEST_MODEL: DecisionModelBundleIdentity = Object.freeze({
  ...BUNDLED_DECISION_MODEL,
  files: Object.freeze(
    BUNDLED_DECISION_MODEL.files.map((file) => {
      const body = Buffer.from(`fixture:${file.path}`, "utf8");
      return Object.freeze({
        ...file,
        maxBytes: body.byteLength,
        sha256: createHash("sha256").update(body).digest("hex"),
      });
    }),
  ),
});

afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true });
});

describe("bundled decision model catalog", () => {
  it("pins the OpenJev model revision and every required artifact", () => {
    expect(BUNDLED_DECISION_MODEL.modelId).toBe("kev-0.6b");
    expect(BUNDLED_DECISION_MODEL.revision).toMatch(/^[a-f0-9]{40}$/u);
    expect(BUNDLED_DECISION_MODEL.files.map(({ path }) => path)).toEqual([
      "added_tokens.json",
      "config.json",
      "merges.txt",
      "onnx/model_q4.onnx",
      "onnx/model_q4.onnx_data",
      "special_tokens_map.json",
      "tokenizer.json",
      "tokenizer_config.json",
      "vocab.json",
    ]);
    for (const file of BUNDLED_DECISION_MODEL.files) {
      expect(file.sha256).toMatch(/^[a-f0-9]{64}$/u);
      expect(file.maxBytes).toBeGreaterThan(0);
      expect(bundledDecisionModelFileUrl(file)).toBe(
        `https://huggingface.co/onnx-community/kev-0.6b-ONNX/resolve/${BUNDLED_DECISION_MODEL.revision}/${file.path}`,
      );
    }
  });
});

describe("bundled decision model installer", () => {
  it("downloads, verifies, atomically activates, and then reuses the pinned bundle", async () => {
    const fixture = await createFixture();
    const fetcher = vi.fn(async (input: string | URL | Request) => responseFor(input));
    const progress: number[] = [];

    await expect(
      installDecisionModelBundle(fixture.storage, TEST_MODEL, {
        operationId: "decision-install-1",
        signal: new AbortController().signal,
        fetcher,
        acquireProcessLock: fixture.acquireProcessLock,
        onProgress: ({ downloadedBytes }) => progress.push(downloadedBytes),
      }),
    ).resolves.toEqual({ kind: "installed", modelPath: fixture.activePath });

    expect(fetcher).toHaveBeenCalledTimes(BUNDLED_DECISION_MODEL.files.length);
    expect(progress.at(-1)).toBe(totalFixtureBytes());
    await expect(
      inspectDecisionModelBundle(fixture.storage, TEST_MODEL, new AbortController().signal),
    ).resolves.toEqual({ kind: "present", modelPath: fixture.activePath });

    fetcher.mockClear();
    await expect(
      installDecisionModelBundle(fixture.storage, TEST_MODEL, {
        operationId: "decision-install-2",
        signal: new AbortController().signal,
        fetcher,
        acquireProcessLock: fixture.acquireProcessLock,
      }),
    ).resolves.toEqual({ kind: "already_installed", modelPath: fixture.activePath });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("does not activate an incomplete or hash-mismatched bundle and cleans staging", async () => {
    const fixture = await createFixture();
    const fetcher = vi.fn(async (input: string | URL | Request) => {
      const url = requestUrl(input);
      const file = TEST_MODEL.files.find((candidate) => url.endsWith(`/${candidate.path}`));
      if (file?.path === "config.json") return sizedResponse("x".repeat(file.maxBytes));
      return responseFor(input);
    });

    await expect(
      installDecisionModelBundle(fixture.storage, TEST_MODEL, {
        operationId: "decision-install-corrupt",
        signal: new AbortController().signal,
        fetcher,
        acquireProcessLock: fixture.acquireProcessLock,
      }),
    ).rejects.toThrow("decision_model_integrity_failed");
    expect(fixture.activations()).toBe(0);
    expect(fixture.cleanups()).toBe(1);
  });

  it("recovers a corrupted active bundle while holding the process lock", async () => {
    const fixture = await createFixture();
    await fixture.seedCorruptedBundle();

    await expect(
      installDecisionModelBundle(fixture.storage, TEST_MODEL, {
        operationId: "decision-install-recover",
        signal: new AbortController().signal,
        fetcher: async (input) => responseFor(input),
        acquireProcessLock: fixture.acquireProcessLock,
      }),
    ).resolves.toEqual({ kind: "installed", modelPath: fixture.activePath });
    expect(fixture.lockAcquisitions()).toBe(1);
  });

  it("accepts chunked or decoded responses without a length header under exact manifest bounds", async () => {
    const fixture = await createFixture();
    await expect(
      installDecisionModelBundle(fixture.storage, TEST_MODEL, {
        operationId: "decision-install-chunked",
        signal: new AbortController().signal,
        fetcher: async (input) => {
          const url = requestUrl(input);
          const file = TEST_MODEL.files.find((candidate) => url.endsWith(`/${candidate.path}`));
          return file === undefined
            ? new Response(null, { status: 404 })
            : new Response(fixtureBody(file.path), { status: 200 });
        },
        acquireProcessLock: fixture.acquireProcessLock,
      }),
    ).resolves.toMatchObject({ kind: "installed" });
  });

  it.each([
    "http://127.0.0.1/internal-model",
    "https://169.254.169.254/latest/meta-data",
    "https://example.com/untrusted-model",
    "http://cdn-lfs.huggingface.co/downgraded-model",
  ])(
    "rejects an unsafe model redirect before making the redirected request: %s",
    async (location) => {
      const fixture = await createFixture();
      const fetcher = vi.fn(async () =>
        fetcher.mock.calls.length === 1
          ? new Response(null, { status: 302, headers: { location } })
          : sizedResponse("unexpected"),
      );

      await expect(
        installDecisionModelBundle(fixture.storage, TEST_MODEL, {
          operationId: `decision-install-redirect-${String(roots.length)}`,
          signal: new AbortController().signal,
          fetcher,
          acquireProcessLock: fixture.acquireProcessLock,
        }),
      ).rejects.toThrow("decision_model_download_redirect_rejected");
      expect(fetcher).toHaveBeenCalledOnce();
      expect(fixture.activations()).toBe(0);
    },
  );
});

const fixtureBody = (path: string): Buffer => Buffer.from(`fixture:${path}`, "utf8");

const totalFixtureBytes = (): number =>
  TEST_MODEL.files.reduce((total, file) => total + fixtureBody(file.path).byteLength, 0);

const responseFor = (input: string | URL | Request): Response => {
  const url = requestUrl(input);
  const file = TEST_MODEL.files.find((candidate) => url.endsWith(`/${candidate.path}`));
  if (file === undefined) return new Response(null, { status: 404 });
  return sizedResponse(fixtureBody(file.path));
};

const sizedResponse = (body: string | Uint8Array): Response => {
  const bytes = typeof body === "string" ? Buffer.from(body, "utf8") : Buffer.from(body);
  return new Response(bytes, {
    status: 200,
    headers: { "content-length": String(bytes.byteLength) },
  });
};

async function createFixture(): Promise<{
  readonly storage: DecisionModelBundleStoragePort;
  readonly activePath: string;
  readonly acquireProcessLock: (
    target: string,
    signal: AbortSignal,
  ) => Promise<() => Promise<void>>;
  readonly activations: () => number;
  readonly cleanups: () => number;
  readonly lockAcquisitions: () => number;
  readonly seedCorruptedBundle: () => Promise<void>;
}> {
  const root = await mkdtemp(join(tmpdir(), "lyonbrown-decision-installer-"));
  roots.push(root);
  const activePath = join(root, "active");
  let active = false;
  let stagingSequence = 0;
  let activationCount = 0;
  let cleanupCount = 0;
  let lockCount = 0;

  const storage: DecisionModelBundleStoragePort = {
    async acquireInstallLockAnchor(
      _identity: DecisionModelBundleIdentity,
      _signal: AbortSignal,
    ): Promise<DecisionModelBundleInstallLock> {
      return {
        async withLockTarget<T>(use: (target: string) => Promise<T>): Promise<T> {
          return use(join(root, "install.lock"));
        },
        release: () => Promise.resolve(),
      };
    },
    async createStagingBundle(
      identity: DecisionModelBundleIdentity,
      _operationId: string,
      _signal: AbortSignal,
    ): Promise<DecisionModelStagingBundle> {
      const stagingPath = join(root, `staging-${String(stagingSequence++)}`);
      for (const file of identity.files)
        await mkdir(dirname(join(stagingPath, file.path)), { recursive: true });
      return {
        async withDirectoryPath<T>(use: (path: string) => Promise<T>): Promise<T> {
          return use(stagingPath);
        },
        async activate(): Promise<void> {
          await rm(activePath, { force: true, recursive: true });
          await mkdir(activePath, { recursive: true });
          for (const file of identity.files) {
            const source = join(stagingPath, file.path);
            const destination = join(activePath, file.path);
            await mkdir(dirname(destination), { recursive: true });
            await writeFile(destination, await readFile(source));
          }
          active = true;
          activationCount += 1;
        },
        async cleanup(): Promise<void> {
          cleanupCount += 1;
          await rm(stagingPath, { force: true, recursive: true });
        },
      };
    },
    async withInstalledBundle<T>(
      _identity: DecisionModelBundleIdentity,
      _signal: AbortSignal,
      use: (path: string) => Promise<T>,
    ) {
      return active ? { kind: "present", value: await use(activePath) } : { kind: "missing" };
    },
  };

  return {
    storage,
    activePath,
    acquireProcessLock: async (_target, signal) => {
      if (signal.aborted) throw new Error("decision_model_install_cancelled");
      lockCount += 1;
      return async () => undefined;
    },
    activations: () => activationCount,
    cleanups: () => cleanupCount,
    lockAcquisitions: () => lockCount,
    seedCorruptedBundle: async () => {
      active = true;
      for (const file of TEST_MODEL.files) {
        const path = join(activePath, file.path);
        await mkdir(dirname(path), { recursive: true });
        await writeFile(path, fixtureBody(file.path));
      }
      const firstFile = TEST_MODEL.files[0];
      if (firstFile === undefined) throw new Error("test model is empty");
      const target = join(activePath, firstFile.path);
      await writeFile(target, "corrupted", "utf8");
    },
  };
}

function requestUrl(input: string | URL | Request): string {
  return typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
}
