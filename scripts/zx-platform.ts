export type WindowsShellSelector = () => unknown;

export function configureZxPlatform(
  platform: NodeJS.Platform,
  usePowerShell: WindowsShellSelector,
): void {
  if (platform === "win32") {
    usePowerShell();
  }
}
