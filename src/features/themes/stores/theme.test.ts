import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ listThemes: vi.fn(), getActiveTheme: vi.fn(), getTheme: vi.fn(), settings: vi.fn(), setActiveTheme: vi.fn(), importTheme: vi.fn(), removeTheme: vi.fn(), exportTheme: vi.fn() }));
vi.mock("../../../services/backend", () => ({ backend: mocks }));
import { useThemeStore } from "./theme";

const active = { id: "orbit-dark", name: "Orbit Dark", version: "1.0.0", author: "Orbit Team", description: "Padrão", type: "dark" as const, orbitVersion: ">=0.1.2", previewUrl: null, source: "builtin" as const, compatible: true, tokens: { colors: { background: "#090b10", surface: "#11141b", surfaceElevated: "#1c202a", primary: "#755be9", secondary: "#4cc9f0", text: "#fff", textMuted: "#777", border: "#222", success: "#4ade80", warning: "#facc15", error: "#f87171" }, radius: { small: "6px", medium: "10px", large: "16px" }, spacing: { unit: "4px" }, typography: { fontFamily: "Inter", headingWeight: 700, bodyWeight: 400 }, effects: { blur: "12px", shadow: "none" } } };

describe("theme store", () => {
  beforeEach(() => { vi.clearAllMocks(); vi.stubGlobal("document", { documentElement: { style: { setProperty: vi.fn() }, dataset: {} } }); vi.stubGlobal("window", { matchMedia: () => ({ matches: false }) }); mocks.settings.mockResolvedValue({ themeMode: "manual" }); useThemeStore.setState({ themes: [], active: null, status: "idle", error: null, loaded: false }); });
  it("loads and applies the persisted theme dynamically", async () => { mocks.listThemes.mockResolvedValue([active]); mocks.getActiveTheme.mockResolvedValue(active); await useThemeStore.getState().load(); expect(useThemeStore.getState().active?.id).toBe("orbit-dark"); expect(document.documentElement.style.setProperty).toHaveBeenCalledWith("--orbit-color-primary", "#755be9"); });
  it("persists a selected theme without a reload", async () => { const light = { ...active, id: "orbit-light", type: "light" as const }; mocks.setActiveTheme.mockResolvedValue(light); await useThemeStore.getState().apply("orbit-light"); expect(mocks.setActiveTheme).toHaveBeenCalledWith("orbit-light"); expect(useThemeStore.getState().active?.id).toBe("orbit-light"); });
  it("keeps an understandable error after a failed import", async () => { mocks.importTheme.mockRejectedValue(new Error("Manifest inválido")); await useThemeStore.getState().importFile("/tmp/bad.orbit-theme"); expect(useThemeStore.getState().error).toBe("Manifest inválido"); });
});
