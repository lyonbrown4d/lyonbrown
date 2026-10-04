import { describe, expect, it, vi } from "vitest";
import { NativeFailure } from "../src/result.js";
import { api } from "../src/windows/api.js";
import { ERRORS } from "../src/windows/constants.js";
import { openSecureDirectory } from "../src/windows/directory.js";
import {
  failureForLastError,
  failureForNtStatus,
  OwnedHandle,
  validateObjectType,
} from "../src/windows/handles.js";

describe("Windows handle boundary", () => {
  it.runIf(process.platform === "win32")("rejects non-canonical run-directory input", () => {
    expect(() => openSecureDirectory("relative\\run")).toThrow(NativeFailure);
    expect(() => openSecureDirectory("C:\\temp\\..\\run")).toThrow(NativeFailure);
    expect(() => openSecureDirectory("C:\\temp\0run")).toThrow(NativeFailure);
  });

  it("coarsens Win32 errors without retaining native data", () => {
    expect(failureForLastError(ERRORS.fileNotFound).failureCode).toBe("not_found");
    expect(failureForLastError(ERRORS.pathNotFound).failureCode).toBe("not_found");
    expect(failureForLastError(ERRORS.accessDenied).failureCode).toBe("access_denied");
    expect(failureForLastError(ERRORS.sharingViolation).failureCode).toBe("access_denied");
    expect(failureForLastError(87).failureCode).toBe("io");
    expect(failureForNtStatus(0xc000_0022 | 0).failureCode).toBe("access_denied");
    expect(failureForNtStatus(0xc000_0034 | 0).failureCode).toBe("not_found");
    expect(failureForNtStatus(0xc000_003a | 0).failureCode).toBe("not_found");
    expect(failureForNtStatus(0xc000_050b | 0).failureCode).toBe("reparse_point");
    expect(failureForNtStatus(0xc000_0001 | 0).failureCode).toBe("io");
  });

  it("rejects invalid handles and closes a live handle once", () => {
    expect(() => new OwnedHandle(null)).toThrow(NativeFailure);
    expect(() => new OwnedHandle(0xffff_ffff_ffff_ffffn)).toThrow(NativeFailure);
    const close = vi.spyOn(api, "CloseHandle").mockImplementation(() => 1);
    const handle = new OwnedHandle(123n);
    expect(handle.borrow()).toBe(123n);
    handle.close();
    handle.close();
    expect(close).toHaveBeenCalledOnce();
    expect(() => handle.borrow()).toThrow(NativeFailure);
    close.mockRestore();
  });

  it("reports a scrubbed close failure and never closes the handle twice", () => {
    const close = vi.spyOn(api, "CloseHandle").mockImplementation(() => 0);
    const handle = new OwnedHandle(123n);
    expect(() => {
      handle.close();
    }).toThrow(
      expect.objectContaining({ failureCode: "io", message: "native control-plane operation failed" }),
    );
    expect(() => {
      handle.close();
    }).not.toThrow();
    expect(close).toHaveBeenCalledOnce();
    close.mockRestore();
  });

  it("rejects reparse points and object type confusion", () => {
    const getInfo = vi
      .spyOn(api, "GetFileInformationByHandleEx")
      .mockImplementation((...arguments_: unknown[]) => {
        const output = arguments_[2] as Buffer;
        output.writeUInt32LE(0x400, 0);
        return 1;
      });
    const handle = new OwnedHandle(123n);
    expect(() => {
      validateObjectType(handle, "file");
    }).toThrow(expect.objectContaining({ failureCode: "reparse_point" }));

    getInfo.mockImplementation((...arguments_: unknown[]) => {
      const output = arguments_[2] as Buffer;
      output.writeUInt32LE(0x10, 0);
      return 1;
    });
    expect(() => {
      validateObjectType(handle, "file");
    }).toThrow(expect.objectContaining({ failureCode: "wrong_type" }));

    getInfo.mockImplementation((...arguments_: unknown[]) => {
      const output = arguments_[2] as Buffer;
      output.writeUInt32LE(0, 0);
      return 1;
    });
    expect(() => {
      validateObjectType(handle, "directory");
    }).toThrow(expect.objectContaining({ failureCode: "not_directory" }));

    getInfo.mockReturnValue(0);
    expect(() => {
      validateObjectType(handle, "file");
    }).toThrow(NativeFailure);
    getInfo.mockRestore();
    vi.spyOn(api, "CloseHandle").mockImplementation(() => 1);
    handle.close();
    vi.restoreAllMocks();
  });
});
