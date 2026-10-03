export class EventStoreError extends Error {
  public constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = new.target.name;
  }
}

export class InvalidStoreOptionsError extends EventStoreError {}

export class PathSecurityError extends EventStoreError {}

export class PayloadValidationError extends EventStoreError {}

export class CorruptEventStoreError extends EventStoreError {}

export class EventStoreClosedError extends EventStoreError {
  public constructor() {
    super("The event store is closed");
  }
}

export class RevisionConflictError extends EventStoreError {
  public constructor(
    public readonly sessionId: string,
    public readonly expectedRevision: bigint,
    public readonly actualRevision: bigint,
  ) {
    super(
      `Revision conflict for session ${JSON.stringify(sessionId)}: expected ${expectedRevision.toString()}, actual ${actualRevision.toString()}`,
    );
  }
}

export class EventAlreadyExistsError extends EventStoreError {
  public constructor(
    public readonly eventId: string,
    options?: ErrorOptions,
  ) {
    super(`Event ${JSON.stringify(eventId)} already exists`, options);
  }
}
