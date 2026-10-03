import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { deriveScope, deriveScopeFromIdentity, encodeWorkspaceIdentity } from "../src/index.js";
import { FakePlatformSecurity, INODE_IDENTITY } from "./test-support.js";

interface IdentityVector {
  readonly name: string;
  readonly identity:
    | { readonly kind: "inode"; readonly device_id: string; readonly inode_number: string }
    | {
        readonly kind: "windows-low-res";
        readonly volume_serial_number: string;
        readonly file_index: string;
      }
    | {
        readonly kind: "windows-high-res";
        readonly volume_serial_number: string;
        readonly file_id: string;
      };
  readonly identity_hex: string;
  readonly config_fingerprint: string;
  readonly scope_id: string;
}

function toIdentity(vector: IdentityVector) {
  const identity = vector.identity;
  switch (identity.kind) {
    case "inode":
      return {
        kind: identity.kind,
        deviceId: BigInt(identity.device_id),
        inodeNumber: BigInt(identity.inode_number),
      } as const;
    case "windows-low-res":
      return {
        kind: identity.kind,
        volumeSerialNumber: BigInt(identity.volume_serial_number),
        fileIndex: BigInt(identity.file_index),
      } as const;
    case "windows-high-res":
      return {
        kind: identity.kind,
        volumeSerialNumber: BigInt(identity.volume_serial_number),
        fileId: BigInt(identity.file_id),
      } as const;
  }
}

describe("Rust workspace identity parity", () => {
  it("consumes the Rust-owned fixed vectors", async () => {
    const fixturePath = fileURLToPath(
      new URL("./fixtures/rust-workspace-identity-vectors.json", import.meta.url),
    );
    const vectors = JSON.parse(await readFile(fixturePath, "utf8")) as IdentityVector[];
    for (const vector of vectors) {
      const encoded = encodeWorkspaceIdentity(toIdentity(vector));
      expect(Buffer.from(encoded).toString("hex"), vector.name).toBe(vector.identity_hex);
      expect(deriveScopeFromIdentity(encoded, vector.config_fingerprint), vector.name).toBe(
        vector.scope_id,
      );
    }
  });

  it("is stable across rename, case, and separator spellings supplied by the provider", async () => {
    const security = new FakePlatformSecurity();
    const canonicalPath = "C:\\Canonical\\Agent";
    for (const path of ["C:\\Work\\Agent", "c:/work/agent", "C:\\Renamed\\Agent"])
      security.setIdentity(path, INODE_IDENTITY, canonicalPath);
    const scopes = await Promise.all(
      ["C:\\Work\\Agent", "c:/work/agent", "C:\\Renamed\\Agent"].map((path) =>
        deriveScope(path, "config-v1", security, "win32"),
      ),
    );
    expect(new Set(scopes.map(({ scopeId }) => scopeId))).toHaveLength(1);
    expect(new Set(scopes.map(({ canonicalWorkspace }) => canonicalWorkspace))).toEqual(
      new Set([canonicalPath]),
    );
  });

  it.each(["relative", "bad\0path", "C:\\bad\ud800"])(
    "rejects invalid native canonical path %j",
    async (canonicalPath) => {
      const security = new FakePlatformSecurity();
      security.setIdentity("C:\\Alias", INODE_IDENTITY, canonicalPath);
      await expect(deriveScope("C:\\Alias", "config", security, "win32")).rejects.toMatchObject({
        code: "PLATFORM_SECURITY_FAILURE",
      });
    },
  );

  it("changes scope when the effective config fingerprint changes", async () => {
    const security = new FakePlatformSecurity();
    security.setIdentity("/workspace", INODE_IDENTITY);
    const first = await deriveScope("/workspace", "config-a", security, "linux");
    const second = await deriveScope("/workspace", "config-b", security, "linux");
    expect(first.scopeId).not.toBe(second.scopeId);
  });

  it("rejects workspace identity fields outside their unsigned width", () => {
    expect(() =>
      encodeWorkspaceIdentity({ kind: "inode", deviceId: -1n, inodeNumber: 1n }),
    ).toThrow();
    expect(() =>
      encodeWorkspaceIdentity({
        kind: "windows-low-res",
        volumeSerialNumber: 1n << 32n,
        fileIndex: 1n,
      }),
    ).toThrow();
  });

  it.each(["relative", "../workspace", "bad\0path", "bad\ud800"])(
    "rejects invalid workspace %j before invoking the provider",
    async (workspace) => {
      const security = new FakePlatformSecurity();
      await expect(deriveScope(workspace, "config", security, "linux")).rejects.toBeInstanceOf(
        Error,
      );
      expect(security.operations).toEqual([]);
    },
  );

  it.each(["", "  ", "bad\0config", "bad\ud800"])("rejects invalid config %j", async (config) => {
    const security = new FakePlatformSecurity();
    security.setIdentity("/workspace", INODE_IDENTITY);
    await expect(deriveScope("/workspace", config, security, "linux")).rejects.toMatchObject({
      code: "INVALID_CONFIG_FINGERPRINT",
    });
  });
});
