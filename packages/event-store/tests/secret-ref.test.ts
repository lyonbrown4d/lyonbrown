import { describe, expect, it } from "vitest";

import { PayloadValidationError, encodeEventPayload } from "../src/index.js";

describe("SecretRef persistence boundary", () => {
  it.each([
    { secretRef: "env:ANTHROPIC_API_KEY" },
    { kind: "secret-ref", reference: "file:github" },
    { nested: [{ secretRef: "keychain:release" }] },
  ])("accepts only closed logical-reference shapes", (payload) => {
    expect(() => encodeEventPayload(payload)).not.toThrow();
  });

  it.each([
    { secretRef: "env:TOKEN", resolvedValue: "plaintext" },
    { secretRef: "env:TOKEN", value: "plaintext" },
    { secretRef: "env:TOKEN", token: "plaintext" },
    { secretRef: "env:TOKEN", custom: "plaintext" },
    { Secret_Ref: "env:TOKEN" },
    { customSecretRef: "env:TOKEN" },
    { kind: "SECRET_REF", reference: "env:TOKEN" },
    { kind: "secret-ref", reference: "env:TOKEN", material: "plaintext" },
    { secretRef: "not-a-logical-reference" },
    { nested: { deeper: [{ secretRef: "env:TOKEN", PlainText: "leak" }] } },
    { apiKey: "plaintext" },
    { ACCESS_TOKEN: "plaintext" },
    { password: "plaintext" },
    { private_key: "plaintext" },
    { credential: "plaintext" },
    { client_secret: "plaintext" },
    { BearerToken: "plaintext" },
    { rawSecret: "plaintext" },
    { githubToken: "plaintext" },
    { deploymentSecret: "plaintext" },
  ])("rejects secret material or SecretRef schema bypasses", (payload) => {
    expect(() => encodeEventPayload(payload)).toThrow(PayloadValidationError);
  });

  it.each([
    { value: "ordinary domain value" },
    { tokenUsage: { input: 12, output: 4 } },
    { materialName: "steel" },
    { publicKey: "not secret material" },
  ])("does not reject documented non-secret lookalikes", (payload) => {
    expect(() => encodeEventPayload(payload)).not.toThrow();
  });
});
