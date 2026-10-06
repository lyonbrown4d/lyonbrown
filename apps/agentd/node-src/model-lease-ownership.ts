import type { LocalModelLease } from "@lyonbrown/local-model";

export class ModelLeaseOwnership {
  private activeLease: LocalModelLease | undefined;
  private readonly retiredLeases = new Set<LocalModelLease>();

  active(): LocalModelLease | undefined {
    return this.activeLease;
  }

  accept(lease: LocalModelLease): void {
    this.activeLease = lease;
  }

  async retire(lease: LocalModelLease): Promise<void> {
    if (this.activeLease === lease) this.activeLease = undefined;
    this.retiredLeases.add(lease);
    try {
      await lease.dispose();
      this.retiredLeases.delete(lease);
    } catch {
      // Keep ownership so shutdown can retry and aggregate a persistent failure.
    }
  }

  owned(): readonly LocalModelLease[] {
    return this.activeLease === undefined
      ? [...this.retiredLeases]
      : [this.activeLease, ...this.retiredLeases];
  }
}
