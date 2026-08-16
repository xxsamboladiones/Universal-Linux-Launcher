export type ThemeType = "dark" | "light";
export type ThemeSource = "builtin" | "external";
export interface ThemeTokens {
  colors: Record<
    | "background"
    | "surface"
    | "surfaceElevated"
    | "primary"
    | "secondary"
    | "text"
    | "textMuted"
    | "border"
    | "success"
    | "warning"
    | "error",
    string
  > & {
    accent?: string;
    primaryForeground?: string;
    secondaryForeground?: string;
    accentForeground?: string;
  };
  radius: { small: string; medium: string; large: string };
  spacing: { unit: string };
  typography: { fontFamily: string; headingWeight: number; bodyWeight: number };
  effects: { blur: string; shadow: string };
}
export interface ThemeSummary {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  type: ThemeType;
  orbitVersion: string;
  previewUrl: string | null;
  source: ThemeSource;
  compatible: boolean;
}
export interface ThemeDetails extends ThemeSummary {
  tokens: ThemeTokens;
}
export interface ProviderStatus {
  provider: string;
  available: boolean;
  version: string | null;
}
export interface AutomaticTheme {
  palette: Record<string, string | boolean>;
  tokens: ThemeTokens;
  source: string;
  wallpaperPath: string;
  paletteHash: string;
}
