import { isFullyQualifiedLocalPath } from "@lyonbrown/decision-engine/internal/local-path";

export interface LocalOpenJevConfiguration {
  /** Absolute path to an already verified local model directory. */
  readonly modelPath: string;
  readonly modelId: string;
  /** Pinned upstream model commit. */
  readonly modelRevision: string;
}

const MODEL_ID = /^[a-z0-9][a-z0-9._-]{0,127}$/u;
const MODEL_REVISION = /^[a-f0-9]{40}$/u;

export function isValidLocalOpenJevConfiguration(
  configuration: LocalOpenJevConfiguration,
): boolean {
  return (
    isFullyQualifiedLocalPath(configuration.modelPath) &&
    MODEL_ID.test(configuration.modelId) &&
    MODEL_REVISION.test(configuration.modelRevision)
  );
}
