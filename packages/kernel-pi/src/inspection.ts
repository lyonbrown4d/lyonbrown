import { types } from "node:util";

export type InspectionFailure = "proxy" | "too_many_keys" | "symbol_key" | "accessor";

export interface OwnDataProperty {
  readonly key: string;
  readonly value: unknown;
  readonly enumerable: boolean;
}

export function inspectOwnDataProperties(
  value: object,
  maximumKeys: number,
  fail: (failure: InspectionFailure) => Error,
): readonly OwnDataProperty[] {
  if (types.isProxy(value)) throw fail("proxy");
  const keys = Reflect.ownKeys(value);
  if (keys.length > maximumKeys) throw fail("too_many_keys");
  const properties: OwnDataProperty[] = [];
  for (const key of keys) {
    if (typeof key !== "string") throw fail("symbol_key");
    // Reflect.ownKeys and descriptor lookup are synchronous; a non-Proxy own key cannot disappear here.
    // eslint-disable-next-line @typescript-eslint/non-nullable-type-assertion-style
    const descriptor = Object.getOwnPropertyDescriptor(value, key) as PropertyDescriptor;
    if (!("value" in descriptor)) throw fail("accessor");
    properties.push({ key, value: descriptor.value, enumerable: Boolean(descriptor.enumerable) });
  }
  return properties;
}
