export type ItemKind = "game" | "application" | "script" | "custom";
export type Provider =
  | "steam"
  | "epic"
  | "gog"
  | "battlenet"
  | "desktop"
  | "flatpak"
  | "appimage"
  | "custom";
export interface GamescopeConfig {
  enabled: boolean;
  width: number | null;
  height: number | null;
  outputWidth: number | null;
  outputHeight: number | null;
  fps: number | null;
  fullscreen: boolean;
  upscaler: string | null;
}
export interface CompatibilityConfig {
  runtimeId: string | null;
  prefixPath: string | null;
  steamOverlay: boolean;
  gamemode: boolean;
  mangohud: boolean;
  gamescope: GamescopeConfig;
  dxvk: boolean;
  vkd3d: boolean;
}
export interface RuntimeInfo {
  id: string;
  name: string;
  family: "wine" | "proton";
  path: string;
  managed: boolean;
}
export interface CompatibilityOverview {
  runtimes: RuntimeInfo[];
  gamemode: boolean;
  mangohud: boolean;
  gamescope: boolean;
  dxvk: boolean;
  vkd3d: boolean;
  sessionType: string;
  desktop: string;
  wayland: boolean;
  terminal: string | null;
  prefixRoot: string;
}
export interface LibraryItem {
  id: string;
  name: string;
  kind: ItemKind;
  provider: Provider;
  executable: string | null;
  arguments: string[];
  workingDirectory: string | null;
  environment: Record<string, string>;
  icon: string | null;
  cover: string | null;
  background: string | null;
  category: string | null;
  tags: string[];
  favorite: boolean;
  hidden: boolean;
  owned: boolean;
  installed: boolean;
  playCount: number;
  totalPlayTimeSeconds: number;
  lastPlayedAt: string | null;
  createdAt: string;
  updatedAt: string;
  terminal: boolean;
  compatibility: CompatibilityConfig;
}
export interface ItemInput {
  id?: string;
  name: string;
  kind: ItemKind;
  provider: Provider;
  executable: string | null;
  arguments: string[];
  workingDirectory: string | null;
  environment: Record<string, string>;
  icon: string | null;
  category: string | null;
  terminal: boolean;
  compatibility: CompatibilityConfig;
}
export interface ScanReport {
  found: number;
  added: number;
  updated: number;
  unavailable: string[];
  errors: string[];
}
export interface ScanProgress {
  provider: string;
  status: "scanning" | "completed";
  found: number;
}
export interface AppSettings {
  theme: "dark" | "system";
  scanOnStartup: boolean;
  confirmBeforeRemove: boolean;
  preferredTerminal: string | null;
}
