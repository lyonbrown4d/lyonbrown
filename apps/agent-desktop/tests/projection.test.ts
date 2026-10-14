import { expect, test } from "vitest";
import { projectSnapshot } from "@lyonbrown/agent-desktop/internal/main/projection";
import { snapshot } from "@lyonbrown/agent-desktop/test/support";

test("projects serializable conversation state and no transport secrets", () => {
  const state = projectSnapshot(
    snapshot({
      turns: [
        {
          turnId: "t",
          userText: "<script>x</script>",
          assistantText: "hello",
          status: "completed",
          steering: [],
          activities: [],
        },
      ],
    }),
    "reused",
  );
  expect(state.revision).toBe("1");
  expect(state.messages.map((message) => message.text)).toEqual(["<script>x</script>", "hello"]);
  expect(state.canSend).toBe(true);
  expect(() => JSON.stringify(state)).not.toThrow();
});
test("does not allow chat before model setup or during an active turn", () => {
  expect(projectSnapshot(snapshot({ phase: "model-setup-required" }), "spawned").canSend).toBe(
    false,
  );
  expect(projectSnapshot(snapshot({ phase: "streaming" }), "spawned").canCancel).toBe(true);
  expect(projectSnapshot(snapshot({ phase: "streaming" }), "spawned").canSend).toBe(false);
});
test("treats the submitted pre-kernel decision stage as active even if runtime phase is ready", () => {
  const state = projectSnapshot(
    snapshot({
      turns: [
        {
          turnId: "t",
          userText: "hello",
          assistantText: "",
          status: "pending",
          steering: [],
          activities: [],
        },
      ],
    }),
    "reused",
  );
  expect(state.canSend).toBe(false);
  expect(state.canCancel).toBe(true);
});
