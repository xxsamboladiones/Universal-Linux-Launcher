import { create } from "zustand";
import { backend } from "../../../services/backend";
import type { AppSettings } from "../../../types/library";
import type {
  AutomaticTheme,
  ProviderStatus,
  ThemeDetails,
  ThemeSummary,
} from "../types";
import { applyThemeTokens } from "../utils/cssVariables";

type Status =
  | "idle"
  | "loading"
  | "importing"
  | "removing"
  | "exporting"
  | "detecting"
  | "generating"
  | "applying";

export const defaultSettings: AppSettings = {
  theme: "dark",
  activeThemeId: "orbit-dark",
  lastManualThemeId: "orbit-dark",
  themeMode: "manual",
  paletteSource: "automatic",
  wallpaperInfluence: 70,
  automaticColorMode: "automatic",
  automaticUpdate: false,
  manualWallpaperPath: null,
  scanOnStartup: false,
  confirmBeforeRemove: true,
  preferredTerminal: "konsole",
};

interface ThemeState {
  themes: ThemeSummary[];
  active: ThemeDetails | null;
  automatic: AutomaticTheme | null;
  pywal: ProviderStatus | null;
  settings: AppSettings;
  status: Status;
  error: string | null;
  loaded: boolean;
  load(): Promise<void>;
  updateSettings(settings: AppSettings): Promise<void>;
  selectManualTheme(id: string): Promise<void>;
  setThemeMode(mode: AppSettings["themeMode"]): Promise<void>;
  setPaletteSource(source: AppSettings["paletteSource"]): Promise<void>;
  refreshAutomatic(): Promise<void>;
  applyAutomatic(theme: AutomaticTheme): void;
  importFile(path: string): Promise<void>;
  remove(id: string): Promise<void>;
  exportFile(id: string, path: string): Promise<void>;
}

function applyTheme(theme: ThemeDetails, id = theme.id) {
  applyThemeTokens(theme.tokens);
  document.documentElement.dataset.orbitTheme = id;
  document.documentElement.style.colorScheme = theme.type;
}

function systemThemeId() {
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "orbit-light"
    : "orbit-dark";
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  themes: [],
  active: null,
  automatic: null,
  pywal: null,
  settings: defaultSettings,
  status: "idle",
  error: null,
  loaded: false,

  async load() {
    set({ status: "loading", error: null });
    try {
      const [themes, persisted, settings, pywal] = await Promise.all([
        backend.listThemes(),
        backend.getActiveTheme(),
        backend.settings(),
        backend.pywalStatus(),
      ]);
      const normalized = {
        ...settings,
        lastManualThemeId:
          settings.lastManualThemeId || settings.activeThemeId || persisted.id,
      };

      set({
        themes,
        active: persisted,
        settings: normalized,
        pywal,
        status: "idle",
        loaded: true,
      });
      document.documentElement.dataset.theme = normalized.theme;

      if (normalized.themeMode === "automatic") {
        await get().refreshAutomatic();
      } else {
        const id =
          normalized.themeMode === "system"
            ? systemThemeId()
            : normalized.lastManualThemeId;
        let selected = persisted;
        if (id !== persisted.id) {
          try {
            selected = await backend.getTheme(id);
          } catch {
            selected = await backend.getTheme("orbit-dark");
            normalized.activeThemeId = selected.id;
            normalized.lastManualThemeId = selected.id;
            await backend.updateSettings(normalized);
            set({ settings: normalized });
          }
        }
        applyTheme(selected);
        set({ active: selected });
      }
    } catch (error) {
      set({ status: "idle", error: errorMessage(error), loaded: true });
    }
  },

  async updateSettings(settings) {
    const previous = get().settings;
    const normalized = {
      ...settings,
      automaticUpdate:
        settings.paletteSource === "pywal" ? true : settings.automaticUpdate,
    };
    set({ settings: normalized, error: null });
    document.documentElement.dataset.theme = normalized.theme;
    try {
      await backend.updateSettings(normalized);
    } catch (error) {
      set({ settings: previous, error: errorMessage(error) });
      document.documentElement.dataset.theme = previous.theme;
      throw error;
    }
  },

  async selectManualTheme(id) {
    set({ status: "applying", error: null });
    try {
      const theme = await backend.setActiveTheme(id);
      const next = {
        ...get().settings,
        theme: theme.type === "light" ? ("system" as const) : ("dark" as const),
        activeThemeId: theme.id,
        lastManualThemeId: theme.id,
        themeMode: "manual" as const,
      };
      await get().updateSettings(next);
      applyTheme(theme);
      set({ active: theme, automatic: null, status: "idle" });
    } catch (error) {
      set({ status: "idle", error: errorMessage(error) });
    }
  },

  async setThemeMode(themeMode) {
    const previous = get().settings;
    const next = {
      ...previous,
      themeMode,
      theme: themeMode === "system" ? ("system" as const) : previous.theme,
    };
    set({ status: "applying", error: null });
    try {
      await get().updateSettings(next);
      if (themeMode === "automatic") {
        await get().refreshAutomatic();
        return;
      }

      const id =
        themeMode === "system"
          ? systemThemeId()
          : next.lastManualThemeId || next.activeThemeId || "orbit-dark";
      let theme: ThemeDetails;
      try {
        theme = await backend.getTheme(id);
      } catch {
        theme = await backend.getTheme("orbit-dark");
      }
      applyTheme(theme);
      set({ active: theme, automatic: null, status: "idle" });
    } catch (error) {
      set({ settings: previous, status: "idle", error: errorMessage(error) });
    }
  },

  async setPaletteSource(paletteSource) {
    const next = {
      ...get().settings,
      paletteSource,
      automaticUpdate:
        paletteSource === "pywal" ? true : get().settings.automaticUpdate,
    };
    try {
      await get().updateSettings(next);
      if (next.themeMode === "automatic") await get().refreshAutomatic();
    } catch {
      // updateSettings already restored state and exposed a readable error.
    }
  },

  async refreshAutomatic() {
    set({ status: "generating", error: null });
    try {
      const [automatic, pywal] = await Promise.all([
        backend.refreshAutomaticTheme(),
        backend.pywalStatus(),
      ]);
      if (get().settings.themeMode !== "automatic") {
        set({ pywal, status: "idle" });
        return;
      }
      applyThemeTokens(automatic.tokens);
      document.documentElement.dataset.orbitTheme = "automatic";
      document.documentElement.style.colorScheme = automatic.palette.dark
        ? "dark"
        : "light";
      set({ automatic, pywal, status: "idle" });
    } catch (error) {
      try {
        const fallback = await backend.getTheme("orbit-dark");
        applyTheme(fallback);
        set({
          active: fallback,
          automatic: null,
          status: "idle",
          error: `${errorMessage(error)}. O tema padrão foi mantido.`,
        });
      } catch {
        set({ status: "idle", error: errorMessage(error) });
      }
    }
  },

  applyAutomatic(automatic) {
    if (get().settings.themeMode !== "automatic") return;
    applyThemeTokens(automatic.tokens);
    document.documentElement.dataset.orbitTheme = "automatic";
    document.documentElement.style.colorScheme = automatic.palette.dark
      ? "dark"
      : "light";
    set({ automatic, error: null, status: "idle" });
  },

  async importFile(path) {
    set({ status: "importing", error: null });
    try {
      await backend.importTheme(path);
      await get().load();
    } catch (error) {
      set({ status: "idle", error: errorMessage(error) });
    }
  },

  async remove(id) {
    set({ status: "removing", error: null });
    try {
      await backend.removeTheme(id);
      await get().load();
    } catch (error) {
      set({ status: "idle", error: errorMessage(error) });
    }
  },

  async exportFile(id, path) {
    set({ status: "exporting", error: null });
    try {
      await backend.exportTheme(id, path);
      set({ status: "idle" });
    } catch (error) {
      set({ status: "idle", error: errorMessage(error) });
    }
  },
}));

const errorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);
