import type {
  ModelCatalog,
  ModelOption,
  ModelPolicy,
  ModelRoute,
  ModelSelection,
  ModelSetupSnapshot,
  RouteSummary,
} from "@lyonbrown/protocol";

import { clientError } from "@lyonbrown/agent-client/internal/errors";
import {
  disabledReason,
  modelAction,
  modelLocality,
  modelRecommendationReason,
  modelSetupPhase,
  modelSource,
  modelTier,
} from "@lyonbrown/agent-client/internal/snapshot-scalars";
import type {
  ModelCatalogDto,
  ModelOptionDto,
  ModelPolicyDto,
  ModelRouteDto,
  ModelSelectionDto,
  ModelSetupDto,
  RouteSummaryDto,
} from "@lyonbrown/agent-client/internal/types";
import {
  protocolDisplayName,
  protocolId,
  protocolText,
  requiredField,
} from "@lyonbrown/agent-client/internal/validation";

export function mapSelection(value: ModelSelection | undefined): ModelSelectionDto {
  const selection = requiredField("model_selection", value).selection;
  if (selection?.$case === "auto" && selection.auto) return { kind: "auto" };
  if (selection?.$case === "pinned") {
    return { kind: "pinned", ...mapRoute(selection.pinned) };
  }
  throw clientError("protocol", "INVALID_SNAPSHOT", "model_selection is invalid");
}

export function mapRoute(value: ModelRoute | undefined): ModelRouteDto {
  const route = requiredField("model_route", value);
  return {
    provider: protocolId("model_route.provider", route.provider),
    model: protocolId("model_route.model", route.model),
  };
}

export function mapRouteSummary(value: RouteSummary | undefined): RouteSummaryDto | null {
  if (value === undefined) return null;
  return {
    selected: mapRoute(value.selected),
    reason: protocolText("route.reason", value.reason),
    fallbacks: value.fallbacks.map((fallback) => ({
      from: mapRoute(fallback.from),
      to: mapRoute(fallback.to),
      reason: protocolText("route.fallback.reason", fallback.reason),
    })),
  };
}

export function mapModelSetup(value: ModelSetupSnapshot | undefined): ModelSetupDto | null {
  if (value === undefined) return null;
  return {
    source: modelSource(value.source),
    phase: modelSetupPhase(value.phase),
    managedModel:
      value.managedModel === undefined
        ? null
        : {
            id: protocolId("model_setup.managed_model.id", value.managedModel.id),
            displayName:
              value.managedModel.displayName === undefined
                ? null
                : protocolDisplayName(
                    "model_setup.managed_model.display_name",
                    value.managedModel.displayName,
                  ),
            recommendationReason: modelRecommendationReason(
              value.managedModel.recommendationReason,
            ),
            expectedDownloadBytes: value.managedModel.expectedDownloadBytes,
            maxBytes: value.managedModel.maxBytes,
            minAvailableMemoryBytes: value.managedModel.minAvailableMemoryBytes,
            sha256: protocolId("model_setup.managed_model.sha256", value.managedModel.sha256),
          },
    operationId:
      value.operationId === undefined
        ? null
        : protocolId("model_setup.operation_id", value.operationId),
    downloadedBytes: value.downloadedBytes,
    totalBytes: value.totalBytes ?? null,
    failure:
      value.failure === undefined
        ? null
        : {
            code: protocolId("model_setup.failure.code", value.failure.code),
            message: protocolText("model_setup.failure.message", value.failure.message),
            retryable: value.failure.retryable,
          },
    allowedActions: value.allowedActions.map(modelAction),
  };
}

export function mapCatalog(value: ModelCatalog | undefined): ModelCatalogDto | null {
  return value === undefined ? null : { models: value.models.map(mapModelOption) };
}

function mapModelOption(value: ModelOption): ModelOptionDto {
  return {
    route: mapRoute(value.route),
    tier: modelTier(value.tier),
    locality: modelLocality(value.locality),
    contextWindow: value.contextWindow,
    maxOutputTokens: value.maxOutputTokens,
    supportsTools: value.supportsTools,
    supportsStreaming: value.supportsStreaming,
    supportsJson: value.supportsJson,
    supportsThinking: value.supportsThinking,
    selectableForWorker: value.selectableForWorker,
    disabledReason: disabledReason(value.disabledReason),
  };
}

export function mapPolicy(value: ModelPolicy | undefined): ModelPolicyDto | null {
  return value === undefined
    ? null
    : {
        active: mapSelection(value.active),
        pending: value.pending === undefined ? null : mapSelection(value.pending),
      };
}
