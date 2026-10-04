import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../src/windows/api.js";
import { SECURITY } from "../src/windows/constants.js";
import {
  currentUser,
  validateAccessAce,
  verifyPrivateObject,
  withPrivateSecurity,
} from "../src/windows/security.js";

const user = { sid: new Uint8Array([1, 2, 3]), sidString: "S-1-5-18" };

afterEach(() => {
  vi.restoreAllMocks();
});

function mockTokenOpen(): void {
  vi.spyOn(api, "OpenProcessToken").mockImplementation((...arguments_: unknown[]) => {
    (arguments_[2] as unknown[])[0] = 123n;
    return 1;
  });
  vi.spyOn(api, "CloseHandle").mockImplementation(() => 1);
}

function accessAce(flags: number, mask: number = SECURITY.fileAllAccess, type = 0): Buffer {
  const ace = Buffer.alloc(8);
  ace[0] = type;
  ace[1] = flags;
  ace.writeUInt32LE(mask, 4);
  return ace;
}

describe("Windows security failures", () => {
  it("rejects a process token open failure", () => {
    vi.spyOn(api, "OpenProcessToken").mockImplementation(() => 0);
    vi.spyOn(api, "GetLastError").mockImplementation(() => 5);
    expect(() => currentUser()).toThrow(expect.objectContaining({ failureCode: "access_denied" }));
  });

  it.each([0, 65_537])("rejects invalid token information length %i", (length) => {
    mockTokenOpen();
    vi.spyOn(api, "GetTokenInformation").mockImplementation((...arguments_: unknown[]) => {
      (arguments_[4] as unknown[])[0] = length;
      return 0;
    });
    expect(() => currentUser()).toThrow(expect.objectContaining({ failureCode: "io" }));
  });

  it("rejects failure to fetch token information", () => {
    mockTokenOpen();
    let calls = 0;
    vi.spyOn(api, "GetTokenInformation").mockImplementation((...arguments_: unknown[]) => {
      (arguments_[4] as unknown[])[0] = 32;
      calls += 1;
      return calls === 1 ? 0 : 0;
    });
    vi.spyOn(api, "GetLastError").mockImplementation(() => 5);
    expect(() => currentUser()).toThrow(expect.objectContaining({ failureCode: "access_denied" }));
  });

  it.each([0, 33])("rejects invalid SID length %i", (sidLength) => {
    mockTokenOpen();
    let calls = 0;
    vi.spyOn(api, "GetTokenInformation").mockImplementation((...arguments_: unknown[]) => {
      (arguments_[4] as unknown[])[0] = 32;
      calls += 1;
      if (calls === 2) (arguments_[2] as Buffer).writeBigUInt64LE(123n, 0);
      return calls === 2 ? 1 : 0;
    });
    vi.spyOn(api, "GetLengthSid").mockImplementation(() => sidLength);
    expect(() => currentUser()).toThrow(expect.objectContaining({ failureCode: "io" }));
  });

  it("rejects security descriptor conversion failure", () => {
    vi.spyOn(api, "ConvertStringSecurityDescriptorToSecurityDescriptorW").mockImplementation(
      () => 0,
    );
    vi.spyOn(api, "GetLastError").mockImplementation(() => 5);
    expect(() => {
      withPrivateSecurity(user, false, () => undefined);
    }).toThrow(expect.objectContaining({ failureCode: "access_denied" }));
  });

  it.each([
    [5, "access_denied"],
    [87, "io"],
  ])("coarsens GetSecurityInfo status %i", (status, code) => {
    vi.spyOn(api, "GetSecurityInfo").mockImplementation(() => status);
    expect(() => {
      verifyPrivateObject(123n, user, false);
    }).toThrow(expect.objectContaining({ failureCode: code }));
  });

  it("accepts only one exact current-user allow ACE shape", () => {
    expect(() => {
      validateAccessAce(accessAce(0), true, false);
    }).not.toThrow();
    expect(() => {
      validateAccessAce(accessAce(3), true, true);
    }).not.toThrow();
    expect(() => {
      validateAccessAce(Buffer.alloc(7), true, false);
    }).toThrow();
    expect(() => {
      validateAccessAce(accessAce(0, SECURITY.fileAllAccess, 1), true, false);
    }).toThrow();
    expect(() => {
      validateAccessAce(accessAce(3), true, false);
    }).toThrow();
    expect(() => {
      validateAccessAce(accessAce(4), true, false);
    }).toThrow();
    expect(() => {
      validateAccessAce(accessAce(0, 1), true, false);
    }).toThrow();
    expect(() => {
      validateAccessAce(accessAce(0), false, false);
    }).toThrow();
  });
});
