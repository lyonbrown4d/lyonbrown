import { types } from "node:util";

export function isSecretReference(
  value: unknown,
): value is { kind: "secretRef"; reference: string } {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  if (types.isProxy(value)) return false;
  const keys = Reflect.ownKeys(value);
  if (
    keys.length !== 2 ||
    keys.some((key) => typeof key !== "string") ||
    !keys.includes("kind") ||
    !keys.includes("reference")
  ) {
    return false;
  }
  const kind = Object.getOwnPropertyDescriptor(value, "kind");
  const reference = Object.getOwnPropertyDescriptor(value, "reference");
  return (
    kind !== undefined &&
    reference !== undefined &&
    "value" in kind &&
    "value" in reference &&
    kind.value === "secretRef" &&
    typeof reference.value === "string"
  );
}
