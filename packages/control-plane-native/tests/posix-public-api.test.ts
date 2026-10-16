import { chmod, mkdir, mkdtemp, realpath, rm, stat } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import {
  createControlPlaneNativeProvider,
  createNativeEventStorePathVerifier,
  createNativeModelBundleStorageCapability,
  createNativeModelStorageCapability,
} from "@lyonbrown/control-plane-native/internal/index";
import {
  deriveScopedDaemonPaths,
  openSecureRunDirectory,
  acquireLifecycleLock,
} from "@lyonbrown/local-control-plane";
import { openEventStore } from "@lyonbrown/event-store";

describe.runIf(process.platform === "darwin" || process.platform === "linux")(
  "native public composition",
  () => {
    it.runIf(process.platform === "darwin")(
      "permits deny-only ancestor ACLs and rejects grants",
      async () => {
        const scratch = await realpath(
          await mkdtemp(join(tmpdir(), "lyonbrown-native-acl-ancestor-")),
        );
        const parent = join(scratch, "parent");
        try {
          await mkdir(parent, { mode: 0o700 });
          execFileSync("/bin/chmod", ["+a", "everyone deny delete", parent]);
          const directory = await openSecureRunDirectory(
            join(parent, "run"),
            createControlPlaneNativeProvider(),
          );
          await directory.close();
          execFileSync("/bin/chmod", ["-N", parent]);
          execFileSync("/bin/chmod", ["+a", "everyone allow write", parent]);
          await expect(
            openSecureRunDirectory(join(parent, "run"), createControlPlaneNativeProvider()),
          ).rejects.toThrow();
        } finally {
          execFileSync("/bin/chmod", ["-N", parent]);
          await rm(scratch, { recursive: true, force: true });
        }
      },
    );
    it("rejects a private run directory below a writable non-sticky ancestor", async () => {
      const scratch = await realpath(await mkdtemp(join(tmpdir(), "lyonbrown-native-ancestor-")));
      try {
        const parent = join(scratch, "writable");
        const run = join(parent, "run");
        await mkdir(parent, { mode: 0o700 });
        await mkdir(run, { mode: 0o700 });
        await chmod(parent, 0o777);
        await expect(
          openSecureRunDirectory(run, createControlPlaneNativeProvider()),
        ).rejects.toThrow();
        expect((await stat(parent)).mode & 0o777).toBe(0o777);
      } finally {
        await rm(scratch, { recursive: true, force: true });
      }
    });
    it("owns a real scoped daemon directory, SQLite store and model caches", async () => {
      const scratch = await realpath(await mkdtemp(join(tmpdir(), "lyonbrown-native-public-")));
      try {
        const provider = createControlPlaneNativeProvider();
        const paths = await deriveScopedDaemonPaths(
          join(scratch, "home"),
          scratch,
          "public-api-v1",
          provider,
        );
        const directory = await openSecureRunDirectory(paths.runDirectory, provider);
        try {
          const lease = await acquireLifecycleLock(directory);
          expect(lease).not.toBeNull();
          try {
            expect(await acquireLifecycleLock(directory)).toBeNull();
          } finally {
            await lease?.release();
          }
        } finally {
          await directory.close();
        }

        const store = openEventStore({
          dataDirectory: join(scratch, "data"),
          pathVerifier: createNativeEventStorePathVerifier(),
        });
        try {
          const committed = await store.append("session", 0n, [
            { eventId: "event", type: "SessionOpened", payload: {} },
          ]);
          expect(committed[0]?.revision).toBe(1n);
          expect(store.diagnostics().journalMode.toLowerCase()).toBe("wal");
        } finally {
          store.close();
        }

        const signal = new AbortController().signal;
        const model = createNativeModelStorageCapability(join(scratch, "models"));
        expect(
          await model.withInstalledFile(
            {
              modelId: "fixture",
              revision: "a".repeat(40),
              sha256: "b".repeat(64),
              filename: "fixture.gguf",
              maxBytes: 64,
            },
            signal,
            () => {
              throw new Error("missing cache must not invoke callback");
            },
          ),
        ).toEqual({ kind: "missing" });
        const bundle = createNativeModelBundleStorageCapability(join(scratch, "bundles"));
        expect(
          await bundle.withInstalledBundle(
            {
              modelId: "fixture",
              revision: "a".repeat(40),
              files: [{ path: "config.json", sha256: "b".repeat(64), maxBytes: 64 }],
            },
            signal,
            () => {
              throw new Error("missing cache must not invoke callback");
            },
          ),
        ).toEqual({ kind: "missing" });
      } finally {
        await rm(scratch, { recursive: true, force: true });
      }
    });
  },
);
