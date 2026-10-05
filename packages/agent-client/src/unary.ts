import { type Metadata, type ServiceError } from "@grpc/grpc-js";

import { TransportFailure, deadline, publicError, type TransportState } from "./transport.ts";

export interface CapturedTransport {
  readonly state: TransportState;
  readonly generation: number;
}

export type UnaryInvocation<T> = (
  metadata: Metadata,
  deadline: Date,
  callback: (error: ServiceError | null, response: T) => void,
) => void;

export function unaryCall<T>(
  metadata: Metadata,
  timeoutMs: number,
  invoke: UnaryInvocation<T>,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    invoke(metadata, deadline(timeoutMs), (error, response) => {
      if (error !== null) reject(new TransportFailure(error));
      else resolve(response);
    });
  });
}

export async function publicUnaryCall<T>(
  metadata: Metadata,
  timeoutMs: number,
  fence: () => void,
  invoke: UnaryInvocation<T>,
): Promise<T> {
  try {
    const response = await unaryCall(metadata, timeoutMs, invoke);
    fence();
    return response;
  } catch (error: unknown) {
    fence();
    throw publicError(error);
  }
}
