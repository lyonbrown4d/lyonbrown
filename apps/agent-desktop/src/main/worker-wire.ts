import { z } from "zod";
const count = z.string().regex(/^[0-9]{1,20}$/u);
const text = z.string().max(1_048_576);
export const desktopStateSchema = z.strictObject({
  status: z.enum(["connecting", "connected", "disconnected"]),
  source: z.enum(["reused", "spawned"]).nullable(),
  revision: count.nullable(),
  actionToken: z.string().max(100).nullable(),
  phase: z.string().max(64),
  messages: z
    .array(z.strictObject({ id: z.string().max(256), role: z.enum(["user", "assistant"]), text }))
    .max(64),
  canSend: z.boolean(),
  canCancel: z.boolean(),
  approval: z
    .strictObject({ tool: z.string().max(256), risk: z.string().max(64), target: text })
    .nullable(),
  models: z
    .array(
      z.strictObject({
        purpose: z.enum(["chat", "decision"]),
        title: z.string().max(256),
        phase: z.string().max(64),
        downloadedBytes: count,
        totalBytes: count.nullable(),
        actions: z.array(z.enum(["download", "retry", "cancel"])).max(3),
      }),
    )
    .max(2),
  error: z.enum(["RUNTIME_ERROR", "BACKEND_UNAVAILABLE", "CONNECTION_LOST"]).nullable(),
});
export const workerRequestSchema = z.discriminatedUnion("type", [
  z.strictObject({ type: z.literal("connect"), id: count }),
  z.strictObject({ type: z.literal("close"), id: count }),
  z.strictObject({ type: z.literal("action"), id: count, action: z.unknown() }),
]);
export const workerMessageSchema = z.discriminatedUnion("type", [
  z.strictObject({ type: z.literal("state"), state: desktopStateSchema }),
  z.strictObject({
    type: z.literal("reply"),
    id: count,
    result: z.strictObject({ ok: z.boolean(), error: z.string().max(64).nullable() }),
  }),
]);
