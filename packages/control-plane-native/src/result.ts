import type {
  PlatformFailureCode,
  PlatformResult,
} from "@lyonbrown/local-control-plane/native-adapter";

export class NativeFailure extends Error {
  public constructor(public readonly failureCode: PlatformFailureCode) {
    super("native control-plane operation failed");
    this.name = "NativeFailure";
  }
}

export function fail<T>(code: PlatformFailureCode): PlatformResult<T> {
  return { ok: false, code };
}

export function succeed<T>(value: T): PlatformResult<T> {
  return { ok: true, value };
}

export async function capture<T>(operation: () => Promise<T> | T): Promise<PlatformResult<T>> {
  try {
    return succeed(await operation());
  } catch (error) {
    return fail(error instanceof NativeFailure ? error.failureCode : "io");
  }
}
