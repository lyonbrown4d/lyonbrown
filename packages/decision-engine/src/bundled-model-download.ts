import { createWriteStream } from "node:fs";
import { rm } from "node:fs/promises";
import { join } from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";

import pRetry, { AbortError } from "p-retry";

import {
  BUNDLED_DECISION_MODEL,
  bundledDecisionModelFileUrl,
} from "@lyonbrown/decision-engine/internal/bundled-model-catalog";
import type {
  DecisionModelBundleFileIdentity,
  DecisionModelBundleIdentity,
} from "@lyonbrown/decision-engine/internal/model-bundle-storage";

export interface DecisionModelDownloadProgress {
  readonly filePath: string;
  readonly downloadedBytes: number;
  readonly totalBytes: number;
}

const MAX_REDIRECTS = 5;
const ALLOWED_DOWNLOAD_HOSTS = new Set([
  "huggingface.co",
  "cdn-lfs.hf.co",
  "cdn-lfs.huggingface.co",
  "cdn-lfs-us-1.hf.co",
  "cas-bridge.xethub.hf.co",
]);

export async function downloadDecisionModelBundle(
  directoryPath: string,
  model: DecisionModelBundleIdentity,
  signal: AbortSignal,
  fetcher: typeof globalThis.fetch,
  onProgress: (progress: DecisionModelDownloadProgress) => void,
): Promise<void> {
  const totalBytes = model.files.reduce((total, file) => total + file.maxBytes, 0);
  let completedBytes = 0;
  let reportedBytes = 0;
  for (const file of model.files) {
    const targetPath = join(directoryPath, ...file.path.split("/"));
    await pRetry(
      async () => {
        await removePartial(targetPath);
        try {
          await downloadFile(targetPath, model, file, signal, fetcher, (downloadedBytes) => {
            reportedBytes = Math.max(reportedBytes, completedBytes + downloadedBytes);
            onProgress({ filePath: file.path, downloadedBytes: reportedBytes, totalBytes });
          });
        } catch (error) {
          await removePartial(targetPath);
          throw classifyFailure(error, signal);
        }
      },
      {
        retries: 2,
        factor: 2,
        minTimeout: 250,
        maxTimeout: 1_000,
        maxRetryTime: 5_000,
        randomize: true,
        signal,
        unref: true,
      },
    );
    completedBytes += file.maxBytes;
    reportedBytes = completedBytes;
    onProgress({ filePath: file.path, downloadedBytes: completedBytes, totalBytes });
  }
}

async function downloadFile(
  targetPath: string,
  model: DecisionModelBundleIdentity,
  file: DecisionModelBundleFileIdentity,
  signal: AbortSignal,
  fetcher: typeof globalThis.fetch,
  onProgress: (downloadedBytes: number) => void,
): Promise<void> {
  throwIfAborted(signal);
  const response = await fetchWithRedirectPolicy(modelFileUrl(model, file), signal, fetcher);
  if (!response.ok) throw new HttpDownloadError(response.status);
  if (response.body === null) throw new Error("decision_model_download_body_missing");
  const length = response.headers.get("content-length");
  if (length !== null && length !== String(file.maxBytes)) {
    throw new Error(
      `decision_model_download_length_invalid:${file.path}:${length}:${String(file.maxBytes)}`,
    );
  }

  let downloadedBytes = 0;
  const counter = new Transform({
    transform(chunk: Buffer, _encoding, callback) {
      downloadedBytes += chunk.byteLength;
      if (downloadedBytes > file.maxBytes) {
        callback(new Error("decision_model_download_length_invalid"));
        return;
      }
      onProgress(downloadedBytes);
      callback(null, chunk);
    },
  });
  const destination = createWriteStream(targetPath, {
    flags: "wx",
    mode: 0o600,
    signal,
  });
  await pipeline(Readable.fromWeb(response.body), counter, destination, { signal });
  if (downloadedBytes !== file.maxBytes) {
    throw new Error("decision_model_download_length_invalid");
  }
}

async function fetchWithRedirectPolicy(
  initialUrl: string,
  signal: AbortSignal,
  fetcher: typeof globalThis.fetch,
): Promise<Response> {
  let url = validatedDownloadUrl(initialUrl);
  for (let redirects = 0; redirects <= MAX_REDIRECTS; redirects += 1) {
    const response = await fetcher(url, { redirect: "manual", signal });
    if (!isRedirect(response.status)) return response;
    if (redirects === MAX_REDIRECTS) throw redirectRejected();
    const location = response.headers.get("location");
    if (location === null) throw redirectRejected();
    try {
      url = validatedDownloadUrl(new URL(location, url).href);
    } catch (error) {
      if (error instanceof DownloadRedirectError) throw error;
      throw redirectRejected();
    }
  }
  throw redirectRejected();
}

function validatedDownloadUrl(value: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw redirectRejected();
  }
  if (
    url.protocol !== "https:" ||
    (url.port !== "" && url.port !== "443") ||
    url.username !== "" ||
    url.password !== "" ||
    !ALLOWED_DOWNLOAD_HOSTS.has(url.hostname.toLowerCase())
  ) {
    throw redirectRejected();
  }
  return url.href;
}

function isRedirect(status: number): boolean {
  return status === 301 || status === 302 || status === 303 || status === 307 || status === 308;
}

function redirectRejected(): DownloadRedirectError {
  return new DownloadRedirectError();
}

function modelFileUrl(
  model: DecisionModelBundleIdentity,
  file: DecisionModelBundleFileIdentity,
): string {
  if (model === BUNDLED_DECISION_MODEL) return bundledDecisionModelFileUrl(file);
  return `https://huggingface.co/onnx-community/kev-0.6b-ONNX/resolve/${model.revision}/${file.path}`;
}

function classifyFailure(error: unknown, signal: AbortSignal): Error {
  if (signal.aborted) return new AbortError(new Error("decision_model_install_cancelled"));
  if (error instanceof HttpDownloadError && !error.retryable) return new AbortError(error);
  if (
    error instanceof DownloadRedirectError ||
    (error instanceof Error &&
      (error.message.startsWith("decision_model_download_length_invalid") ||
        error.message === "decision_model_download_body_missing"))
  ) {
    return new AbortError(error);
  }
  return error instanceof Error ? error : new Error("decision_model_download_failed");
}

class DownloadRedirectError extends Error {
  public constructor() {
    super("decision_model_download_redirect_rejected");
  }
}

class HttpDownloadError extends Error {
  public readonly retryable: boolean;

  public constructor(status: number) {
    super("decision_model_download_http_failed");
    this.retryable = status === 408 || status === 429 || status >= 500;
  }
}

async function removePartial(path: string): Promise<void> {
  await rm(path, { force: true });
}

function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) throw new Error("decision_model_install_cancelled");
}
