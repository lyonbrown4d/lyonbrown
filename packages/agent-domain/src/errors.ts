export interface DomainErrorDetailsByCode {
  invalid_argument: {
    field?: string;
    reason: string;
    limitBytes?: number;
    key?: string;
  };
  unauthenticated: {
    reason: string;
  };
  failed_precondition: {
    reason: string;
    expected?: string | number | bigint;
    actual?: string | number | bigint;
  };
  stale_server_epoch: {
    expectedServerInstanceId?: string;
    actualServerInstanceId?: string;
  };
  not_found: {
    resource: string;
    id?: string;
  };
  already_exists: {
    resource: string;
    id?: string;
  };
  resource_exhausted: {
    resource: string;
    limitBytes?: number;
    limit?: bigint;
  };
  cancelled: {
    reason?: string;
  };
  deadline_exceeded: {
    deadlineMs?: number;
  };
  internal: {
    reason?: string;
  };
}

export type DomainErrorCode = keyof DomainErrorDetailsByCode;

export class DomainError<C extends DomainErrorCode = DomainErrorCode> extends Error {
  readonly code: C;
  readonly details: Readonly<DomainErrorDetailsByCode[C]>;

  constructor(code: C, message: string, details: DomainErrorDetailsByCode[C]) {
    super(message);
    this.name = "DomainError";
    this.code = code;
    this.details = Object.freeze({ ...details });
  }
}

export function domainError<C extends DomainErrorCode>(
  code: C,
  message: string,
  details: DomainErrorDetailsByCode[C],
): DomainError<C> {
  return new DomainError(code, message, details);
}
