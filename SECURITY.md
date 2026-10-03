# Security Policy

## Temporary Audit Exception

`CVE-2026-93687` is the only accepted audit exception.

- **Scope:** development-only tooling reached through ESLint and `fast-glob`. It processes
  repository-controlled glob patterns and is not part of a production runtime or user-input path.
- **Acceptance reason:** the affected dependency chain cannot currently be upgraded without
  breaking the pinned lint toolchain. The repository-controlled input boundary limits exposure
  while upstream remediation is pending.
- **Owner:** Lyonbrown project maintainers.
- **Review deadline:** 2027-01-15.
- **Removal condition:** remove the exception as soon as upstream publishes a fix and the
  ESLint/`fast-glob` dependency chain can be upgraded to it without breaking the quality gates.

The exception is enforced narrowly through `pnpm.auditConfig.ignoreCves` in `package.json`.
Broad audit suppression flags such as `--ignore-unfixable` are not permitted.
