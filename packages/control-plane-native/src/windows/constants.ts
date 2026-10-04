export const ACCESS = {
  genericRead: 0x8000_0000,
  genericWrite: 0x4000_0000,
  delete: 0x0001_0000,
  readControl: 0x0002_0000,
  fileListDirectory: 0x0000_0001,
  fileTraverse: 0x0000_0020,
  fileReadAttributes: 0x0000_0080,
} as const;

export const SHARE = {
  read: 0x1,
  write: 0x2,
  delete: 0x4,
} as const;

export const CREATION = {
  createNew: 1,
  openExisting: 3,
  openAlways: 4,
} as const;

export const FLAGS = {
  backupSemantics: 0x0200_0000,
  openReparsePoint: 0x0020_0000,
  writeThrough: 0x8000_0000,
} as const;

export const ATTRIBUTES = {
  directory: 0x10,
  normal: 0x80,
  reparsePoint: 0x400,
} as const;

export const ERRORS = {
  fileNotFound: 2,
  pathNotFound: 3,
  accessDenied: 5,
  sharingViolation: 32,
  lockViolation: 33,
  alreadyExists: 183,
} as const;

export const SECURITY = {
  tokenQuery: 0x0008,
  ownerInformation: 0x0000_0001,
  daclInformation: 0x0000_0004,
  protectedDacl: 0x1000,
  seFileObject: 1,
  sddlRevision1: 1,
  accessAllowedAce: 0,
  fileAllAccess: 0x001f_01ff,
} as const;

export const FILE_INFO = {
  rename: 3,
  disposition: 4,
  attributeTag: 9,
  id: 18,
} as const;

export const LOCK = {
  failImmediately: 0x1,
  exclusive: 0x2,
} as const;
