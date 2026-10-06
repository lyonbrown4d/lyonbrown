export interface SurfaceStyles {
  readonly accent: (value: string) => string;
  readonly secondary: (value: string) => string;
  readonly muted: (value: string) => string;
  readonly success: (value: string) => string;
  readonly warning: (value: string) => string;
  readonly danger: (value: string) => string;
  readonly active: (value: string) => string;
}
