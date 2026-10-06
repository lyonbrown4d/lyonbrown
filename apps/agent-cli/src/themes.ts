import type { ThemeId } from "@lyonbrown/ui-model";

export interface ThemePalette {
  readonly background: string;
  readonly surfaceRaised: string;
  readonly surfaceActive: string;
  readonly textPrimary: string;
  readonly textSecondary: string;
  readonly textFaint: string;
  readonly accentPrimary: string;
  readonly accentSecondary: string;
  readonly statusSuccess: string;
  readonly statusWarning: string;
  readonly statusDanger: string;
  readonly borderDefault: string;
  readonly focusRing: string;
}

function palette(values: Omit<ThemePalette, "focusRing">): ThemePalette {
  return { ...values, focusRing: values.accentPrimary };
}

export const THEMES: Readonly<Record<ThemeId, ThemePalette>> = {
  "carbon-mint": palette({
    background: "#090C10",
    surfaceRaised: "#121820",
    surfaceActive: "#18212B",
    textPrimary: "#E8EDF2",
    textSecondary: "#9AA5B1",
    textFaint: "#647080",
    accentPrimary: "#5EEAD4",
    accentSecondary: "#A5B4FC",
    statusSuccess: "#4ADE80",
    statusWarning: "#FBBF24",
    statusDanger: "#FB7185",
    borderDefault: "#273240",
  }),
  "ink-violet": palette({
    background: "#0C0B10",
    surfaceRaised: "#16141D",
    surfaceActive: "#1D1928",
    textPrimary: "#F0EDF5",
    textSecondary: "#A39CAC",
    textFaint: "#6F687A",
    accentPrimary: "#A78BFA",
    accentSecondary: "#7DD3FC",
    statusSuccess: "#5EEAD4",
    statusWarning: "#F6C453",
    statusDanger: "#FB7185",
    borderDefault: "#332D40",
  }),
  "slate-blue": palette({
    background: "#0A0F16",
    surfaceRaised: "#111A24",
    surfaceActive: "#182433",
    textPrimary: "#E6EDF5",
    textSecondary: "#98A6B8",
    textFaint: "#64748B",
    accentPrimary: "#60A5FA",
    accentSecondary: "#67E8F9",
    statusSuccess: "#34D399",
    statusWarning: "#F5B942",
    statusDanger: "#F87171",
    borderDefault: "#2B3A4D",
  }),
  "hearth-teal": palette({
    background: "#0B1113",
    surfaceRaised: "#122024",
    surfaceActive: "#182A2F",
    textPrimary: "#EAE7DF",
    textSecondary: "#A8A39A",
    textFaint: "#716D66",
    accentPrimary: "#E07A5F",
    accentSecondary: "#74C7B8",
    statusSuccess: "#6FCF97",
    statusWarning: "#E9B949",
    statusDanger: "#E76F76",
    borderDefault: "#2A3A3D",
  }),
  "sandstone-espresso": palette({
    background: "#100E0C",
    surfaceRaised: "#1A1713",
    surfaceActive: "#241F19",
    textPrimary: "#EEE9E0",
    textSecondary: "#A9A093",
    textFaint: "#70685E",
    accentPrimary: "#D4A373",
    accentSecondary: "#8FB996",
    statusSuccess: "#7BC8A4",
    statusWarning: "#E8B44F",
    statusDanger: "#E27D75",
    borderDefault: "#3A3229",
  }),
  "graphite-coral": palette({
    background: "#101114",
    surfaceRaised: "#191A1F",
    surfaceActive: "#22242B",
    textPrimary: "#ECEEF2",
    textSecondary: "#9DA3AE",
    textFaint: "#666C78",
    accentPrimary: "#F28C70",
    accentSecondary: "#82A7C9",
    statusSuccess: "#74C69D",
    statusWarning: "#F0B95B",
    statusDanger: "#EF6F78",
    borderDefault: "#30333D",
  }),
};
