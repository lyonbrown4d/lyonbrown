import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "@lyonbrown/control-plane-native/internal/windows/api";
import { SECURITY } from "@lyonbrown/control-plane-native/internal/windows/constants";
import {
  currentUser,
  validateAccessAce,
  verifyPrivateObject,
  withPrivateSecurity,
} from "@lyonbrown/control-plane-native/internal/windows/security";

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

function mockSecurityInfo(dacl: unknown = 456n): void {
  vi.spyOn(api, "GetSecurityInfo").mockImplementation((...arguments_: unknown[]) => {
    (arguments_[3] as unknown[])[0] = 123n;
    (arguments_[5] as unknown[])[0] = dacl;
    (arguments_[7] as unknown[])[0] = 789n;
    return 0;
  });
  vi.spyOn(api, "LocalFree").mockImplementation(() => null);
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

  it("rejects a secure object's wrong owner", () => {
    mockSecurityInfo();
    vi.spyOn(api, "EqualSid").mockImplementation(() => 0);
    expect(() => {
      verifyPrivateObject(123n, user, false);
    }).toThrow(expect.objectContaining({ failureCode: "wrong_owner" }));
  });

  it("rejects a missing DACL", () => {
    mockSecurityInfo(null);
    vi.spyOn(api, "EqualSid").mockImplementation(() => 1);
    expect(() => {
      verifyPrivateObject(123n, user, false);
    }).toThrow(expect.objectContaining({ failureCode: "insecure_acl" }));
  });

  it.each([
    [0, 0, "io"],
    [1, 0, "insecure_acl"],
  ])("rejects unusable descriptor control (%i, %i)", (result, control, code) => {
    mockSecurityInfo();
    vi.spyOn(api, "EqualSid").mockImplementation(() => 1);
    vi.spyOn(api, "GetSecurityDescriptorControl").mockImplementation((...arguments_: unknown[]) => {
      (arguments_[1] as unknown[])[0] = control;
      return result;
    });
    vi.spyOn(api, "GetLastError").mockImplementation(() => 87);
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
