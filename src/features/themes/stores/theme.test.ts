import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSettings } from "../../../types/library";
import type { AutomaticTheme, ThemeDetails } from "../types";

const mocks = vi.hoisted(() => ({
  listThemes: vi.fn(),
  getActiveTheme: vi.fn(),
  getTheme: vi.fn(),
  settings: vi.fn(),
  updateSettings: vi.fn(),
  pywalStatus: vi.fn(),
  setActiveTheme: vi.fn(),
  refreshAutomaticTheme: vi.fn(),
  importTheme: vi.fn(),
  removeTheme: vi.fn(),
  exportTheme: vi.fn(),
}));

vi.mock("../../../services/backend", () => ({ backend: mocks }));

import { defaultSettings, useThemeStore } from "./theme";

const tokens = {
  colors: {
    background: "#090b10",
    surface: "#11141b",
    surfaceElevated: "#1c202a",
    primary: "#755be9",
    secondary: "#4cc9f0",
    text: "#ffffff",
    textMuted: "#777777",
    border: "#222222",
    success: "#4ade80",
    warning: "#facc15",
    error: "#f87171",
  },
  radius: { small: "6px", medium: "10px", large: "16px" },
  spacing: { unit: "4px" },
  typography: {
    fontFamily: "Inter",
    headingWeight: 700,
    bodyWeight: 400,
  },
  effects: { blur: "12px", shadow: "none" },
};

const theme = (id: string, type: "dark" | "light" = "dark"): ThemeDetails => ({
  id,
  name: id,
  version: "1.0.0",
  author: "Orbit Team",
  description: "Tema de teste",
  type,
  orbitVersion: ">=0.1.2",
  previewUrl: null,
  source: "builtin",
  compatible: true,
  tokens,
});

const dark = theme("orbit-dark");
const light = theme("orbit-light", "light");
const midnight = theme("midnight");

const automatic: AutomaticTheme = {
  palette: { dark: true, primary: "#755be9" },
  tokens,
  source: "wal",
  wallpaperPath: "/tmp/wallpaper.png",
  paletteHash: "abc",
};

const settings = (patch: Partial<AppSettings> = {}): AppSettings => ({
  ...defaultSettings,
  ...patch,
});

describe("theme store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("document", {
      documentElement: {
        style: { setProperty: vi.fn(), colorScheme: "" },
        dataset: {},
      },
    });
    vi.stubGlobal("window", {
      matchMedia: () => ({ matches: false }),
    });
    mocks.listThemes.mockResolvedValue([dark, light, midnight]);
    mocks.getActiveTheme.mockResolvedValue(dark);
    mocks.getTheme.mockImplementation(async (id: string) =>
      id === "orbit-light" ? light : id === "midnight" ? midnight : dark,
    );
    mocks.settings.mockResolvedValue(settings());
    mocks.updateSettings.mockResolvedValue(undefined);
    mocks.pywalStatus.mockResolvedValue({
      provider: "wal",
      available: true,
      version: null,
    });
    mocks.refreshAutomaticTheme.mockResolvedValue(automatic);
    useThemeStore.setState({
      themes: [],
      active: null,
      automatic: null,
      pywal: null,
      settings: settings(),
      status: "idle",
      error: null,
      loaded: false,
    });
  });

  it("loads and applies the persisted manual theme dynamically", async () => {
    mocks.getActiveTheme.mockResolvedValue(midnight);
    mocks.settings.mockResolvedValue(
      settings({ activeThemeId: "midnight", lastManualThemeId: "midnight" }),
    );

    await useThemeStore.getState().load();

    expect(useThemeStore.getState().active?.id).toBe("midnight");
    expect(document.documentElement.style.setProperty).toHaveBeenCalledWith(
      "--orbit-color-primary",
      "#755be9",
    );
  });

  it("selects and persists a manual theme without a reload", async () => {
    mocks.setActiveTheme.mockResolvedValue(light);

    await useThemeStore.getState().selectManualTheme("orbit-light");

    expect(mocks.setActiveTheme).toHaveBeenCalledWith("orbit-light");
    expect(mocks.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        themeMode: "manual",
        activeThemeId: "orbit-light",
        lastManualThemeId: "orbit-light",
      }),
    );
    expect(useThemeStore.getState().active?.id).toBe("orbit-light");
  });

  it("applies automatic tokens immediately when entering automatic mode", async () => {
    await useThemeStore.getState().setThemeMode("automatic");

    expect(mocks.refreshAutomaticTheme).toHaveBeenCalledOnce();
    expect(mocks.setActiveTheme).not.toHaveBeenCalled();
    expect(useThemeStore.getState().automatic?.paletteHash).toBe("abc");
    expect(document.documentElement.dataset.orbitTheme).toBe("automatic");
  });

  it("restores the last manual theme after leaving automatic mode", async () => {
    useThemeStore.setState({
      settings: settings({
        themeMode: "automatic",
        activeThemeId: "midnight",
        lastManualThemeId: "midnight",
      }),
      automatic,
    });

    await useThemeStore.getState().setThemeMode("manual");

    expect(mocks.getTheme).toHaveBeenCalledWith("midnight");
    expect(mocks.setActiveTheme).not.toHaveBeenCalled();
    expect(useThemeStore.getState().active?.id).toBe("midnight");
    expect(useThemeStore.getState().automatic).toBeNull();
  });

  it("uses the system theme without overwriting the last manual selection", async () => {
    useThemeStore.setState({
      settings: settings({
        activeThemeId: "midnight",
        lastManualThemeId: "midnight",
      }),
    });

    await useThemeStore.getState().setThemeMode("system");

    expect(mocks.getTheme).toHaveBeenCalledWith("orbit-dark");
    expect(mocks.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        themeMode: "system",
        lastManualThemeId: "midnight",
      }),
    );
    expect(mocks.setActiveTheme).not.toHaveBeenCalled();
  });

  it("does not apply watcher events outside automatic mode", () => {
    useThemeStore.getState().applyAutomatic(automatic);

    expect(useThemeStore.getState().automatic).toBeNull();
    expect(document.documentElement.dataset.orbitTheme).toBeUndefined();
  });

  it("makes Pywal updates implicit and refreshes an active automatic theme", async () => {
    useThemeStore.setState({
      settings: settings({ themeMode: "automatic", automaticUpdate: false }),
    });

    await useThemeStore.getState().setPaletteSource("pywal");

    expect(mocks.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        paletteSource: "pywal",
        automaticUpdate: true,
      }),
    );
    expect(mocks.refreshAutomaticTheme).toHaveBeenCalledOnce();
  });

  it("falls back to Orbit Dark when automatic generation fails", async () => {
    useThemeStore.setState({ settings: settings({ themeMode: "automatic" }) });
    mocks.refreshAutomaticTheme.mockRejectedValue(
      new Error("Wallpaper inválido"),
    );

    await useThemeStore.getState().refreshAutomatic();

    expect(mocks.getTheme).toHaveBeenCalledWith("orbit-dark");
    expect(useThemeStore.getState().active?.id).toBe("orbit-dark");
    expect(useThemeStore.getState().error).toContain("tema padrão foi mantido");
  });

  it("keeps an understandable error after a failed import", async () => {
    mocks.importTheme.mockRejectedValue(new Error("Manifest inválido"));

    await useThemeStore.getState().importFile("/tmp/bad.orbit-theme");

    expect(useThemeStore.getState().error).toBe("Manifest inválido");
  });
});
