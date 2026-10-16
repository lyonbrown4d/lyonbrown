import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

interface PosixConfiguration {
  readonly flags: {
    readonly readOnly: number;
    readonly readWrite: number;
    readonly create: number;
    readonly exclusive: number;
    readonly noFollow: number;
    readonly directory: number;
    readonly closeOnExec: number;
    readonly nonBlock: number;
  };
  readonly libc: string | null;
  readonly workingDirectoryFd: number;
  readonly removeDirectoryFlag: number;
  readonly descriptorRoot: string;
}

export function posixConfiguration(
  platform: NodeJS.Platform,
  architecture: string = process.arch,
): PosixConfiguration {
  if (architecture !== "x64" && architecture !== "arm64") throw new NativeFailure("io");
  if (platform === "darwin") {
    return {
      flags: {
        readOnly: 0,
        readWrite: 2,
        create: 0x200,
        exclusive: 0x800,
        noFollow: 0x100,
        directory: 0x100000,
        closeOnExec: 0x1000000,
        nonBlock: 4,
      },
      libc: "/usr/lib/libSystem.B.dylib",
      workingDirectoryFd: -2,
      removeDirectoryFlag: 0x80,
      descriptorRoot: "/dev/fd",
    };
  }
  if (platform === "linux") {
    return {
      flags: {
        readOnly: 0,
        readWrite: 2,
        create: 0x40,
        exclusive: 0x80,
        noFollow: architecture === "arm64" ? 0x8000 : 0x20000,
        directory: architecture === "arm64" ? 0x4000 : 0x10000,
        closeOnExec: 0x80000,
        nonBlock: 0x800,
      },
      // Resolve the linked system libc, including musl, rather than naming glibc.
      libc: null,
      workingDirectoryFd: -100,
      removeDirectoryFlag: 0x200,
      descriptorRoot: "/proc/self/fd",
    };
  }
  throw new NativeFailure("io");
}
