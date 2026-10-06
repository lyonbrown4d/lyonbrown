import { createWriteStream } from "node:fs";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { setTimeout as delay } from "node:timers/promises";

const REQUEST_RETRY_DELAYS_MS = [250, 1_000] as const;

export interface ModelFileDownloadRequest {
  readonly source: string;
  readonly targetPath: string;
  readonly maxBytes: number;
  readonly signal: AbortSignal;
  readonly onProgress: (progress: {
    readonly downloadedBytes: number;
    readonly totalBytes: number;
  }) => void;
  readonly fetcher?: typeof globalThis.fetch;
}

export const downloadModelFile = async (request: ModelFileDownloadRequest): Promise<void> => {
  throwIfCancelled(request.signal);
  const response = await fetchResponse(request);
  let completed = false;
  try {
    if (!response.ok) throw new Error("model_download_http_failed");
    const totalBytes = parseContentLength(response.headers.get("content-length"), request.maxBytes);
    if (response.body === null) throw new Error("model_download_body_missing");

    let downloadedBytes = 0;
    const progress = new Transform({
      transform(chunk: Buffer, _encoding, callback) {
        if (chunk.byteLength === 0) {
          callback();
          return;
        }
        const nextDownloadedBytes = downloadedBytes + chunk.byteLength;
        if (
          !Number.isSafeInteger(nextDownloadedBytes) ||
          nextDownloadedBytes > totalBytes ||
          nextDownloadedBytes > request.maxBytes
        ) {
          callback(new Error("model_download_length_mismatch"));
          return;
        }
        downloadedBytes = nextDownloadedBytes;
        request.onProgress({ downloadedBytes, totalBytes });
        callback(null, chunk);
      },
    });
    const destination = createWriteStream(request.targetPath, {
      flags: "wx",
      mode: 0o600,
      signal: request.signal,
    });

    await pipeline(Readable.fromWeb(response.body), progress, destination, {
      signal: request.signal,
    });
    throwIfCancelled(request.signal);
    if (downloadedBytes !== totalBytes) throw new Error("model_download_length_mismatch");
    completed = true;
  } catch (error) {
    if (request.signal.aborted) throw new Error("model_download_cancelled", { cause: error });
    throw error;
  } finally {
    if (!completed) await cancelBodyQuietly(response.body);
  }
};

const fetchResponse = async (request: ModelFileDownloadRequest): Promise<Response> => {
  let lastFailure: unknown;
  for (let attempt = 0; attempt <= REQUEST_RETRY_DELAYS_MS.length; attempt += 1) {
    try {
      return await (request.fetcher ?? globalThis.fetch)(request.source, {
        redirect: "follow",
        signal: request.signal,
      });
    } catch (error) {
      if (request.signal.aborted) throw new Error("model_download_cancelled", { cause: error });
      lastFailure = error;
      const retryDelayMs = REQUEST_RETRY_DELAYS_MS[attempt];
      if (retryDelayMs === undefined) break;
      try {
        await delay(retryDelayMs, undefined, { signal: request.signal });
      } catch (delayFailure) {
        throw new Error("model_download_cancelled", { cause: delayFailure });
      }
    }
  }
  throw new Error("model_download_request_failed", { cause: lastFailure });
};

const parseContentLength = (value: string | null, maxBytes: number): number => {
  if (value === null || !/^[1-9]\d*$/u.test(value)) {
    throw new Error("model_download_length_invalid");
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed > maxBytes) {
    throw new Error("model_download_length_invalid");
  }
  return parsed;
};

const cancelBodyQuietly = async (body: ReadableStream<Uint8Array> | null): Promise<void> => {
  try {
    await body?.cancel();
  } catch {
    // The original download failure remains authoritative.
  }
};

const throwIfCancelled = (signal: AbortSignal): void => {
  if (signal.aborted) throw new Error("model_download_cancelled");
};
