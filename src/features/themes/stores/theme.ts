import { create } from "zustand";
import { backend } from "../../../services/backend";
import { applyThemeTokens } from "../utils/cssVariables";
import type { ThemeDetails, ThemeSummary } from "../types";
type Status = "idle" | "loading" | "importing" | "removing" | "exporting";
interface ThemeState { themes: ThemeSummary[]; active: ThemeDetails | null; status: Status; error: string | null; loaded: boolean; load(): Promise<void>; apply(id: string): Promise<void>; importFile(path: string): Promise<void>; remove(id: string): Promise<void>; exportFile(id: string, path: string): Promise<void>; }
export const useThemeStore = create<ThemeState>((set, get) => ({
  themes: [], active: null, status: "idle", error: null, loaded: false,
  async load() { set({ status: "loading", error: null }); try { const [themes, active] = await Promise.all([backend.listThemes(), backend.getActiveTheme()]); applyThemeTokens(active.tokens); document.documentElement.dataset.orbitTheme = active.id; set({ themes, active, status: "idle", loaded: true }); } catch (error) { set({ status: "idle", error: errorMessage(error), loaded: true }); } },
  async apply(id) { set({ status: "loading", error: null }); try { const active = await backend.setActiveTheme(id); applyThemeTokens(active.tokens); document.documentElement.dataset.orbitTheme = active.id; set({ active, status: "idle" }); } catch (error) { set({ status: "idle", error: errorMessage(error) }); } },
  async importFile(path) { set({ status: "importing", error: null }); try { await backend.importTheme(path); await get().load(); } catch (error) { set({ status: "idle", error: errorMessage(error) }); } },
  async remove(id) { set({ status: "removing", error: null }); try { await backend.removeTheme(id); await get().load(); } catch (error) { set({ status: "idle", error: errorMessage(error) }); } },
  async exportFile(id, path) { set({ status: "exporting", error: null }); try { await backend.exportTheme(id, path); set({ status: "idle" }); } catch (error) { set({ status: "idle", error: errorMessage(error) }); } },
}));
const errorMessage = (error: unknown) => error instanceof Error ? error.message : String(error);
