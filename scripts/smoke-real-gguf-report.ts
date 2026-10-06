import type { RuntimeSnapshotDto } from "@lyonbrown/agent-client";

const PROGRESS_REPORT_BYTES = 64n * 1024n * 1024n;

export class SnapshotReporter {
  private reportedSetupPhase: string | undefined;
  private nextProgressReport = 0n;

  public snapshot(snapshot: RuntimeSnapshotDto): void {
    const setup = snapshot.modelSetup;
    if (setup === null) return;
    const phaseChanged = setup.phase !== this.reportedSetupPhase;
    const progressDue = setup.downloadedBytes >= this.nextProgressReport;
    if (!phaseChanged && !progressDue) return;
    this.reportedSetupPhase = setup.phase;
    this.nextProgressReport = setup.downloadedBytes + PROGRESS_REPORT_BYTES;
    this.stage("snapshot", {
      runtimePhase: snapshot.phase,
      setupPhase: setup.phase,
      downloadedBytes: setup.downloadedBytes.toString(),
      totalBytes: setup.totalBytes?.toString() ?? null,
    });
  }

  public stage(stage: string, details: Readonly<Record<string, unknown>> = {}): void {
    process.stdout.write(`${JSON.stringify({ stage, ...details })}\n`);
  }
}

export function safeSnapshot(snapshot: RuntimeSnapshotDto | undefined): unknown {
  if (snapshot === undefined) return null;
  return {
    revision: snapshot.revision.toString(),
    phase: snapshot.phase,
    turnStatuses: snapshot.turns.map((turn) => turn.status),
    modelSetup:
      snapshot.modelSetup === null
        ? null
        : {
            phase: snapshot.modelSetup.phase,
            downloadedBytes: snapshot.modelSetup.downloadedBytes.toString(),
            totalBytes: snapshot.modelSetup.totalBytes?.toString() ?? null,
            hasOperationId: snapshot.modelSetup.operationId !== null,
            failureCode: snapshot.modelSetup.failure?.code ?? null,
          },
    hasLastError: snapshot.lastError !== null,
  };
}
