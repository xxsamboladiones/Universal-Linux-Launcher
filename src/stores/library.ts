import { create } from "zustand";
import { backend } from "../services/backend";
import type { LibraryItem, ScanProgress, ScanReport } from "../types/library";
interface State {
  items: LibraryItem[];
  loading: boolean;
  error: string | null;
  query: string;
  view: "grid" | "list";
  filter: string;
  report: ScanReport | null;
  progress: ScanProgress | null;
  running: Record<string, number>;
  load: () => Promise<void>;
  scan: () => Promise<void>;
  setQuery: (v: string) => void;
  setView: (v: "grid" | "list") => void;
  setFilter: (v: string) => void;
  favorite: (i: LibraryItem) => Promise<void>;
  launch: (i: LibraryItem) => Promise<void>;
  hide: (i: LibraryItem) => Promise<void>;
  remove: (i: LibraryItem) => Promise<void>;
  refreshRunning: () => Promise<void>;
  setProgress: (progress: ScanProgress) => void;
}
const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);
export const useLibrary = create<State>((set, get) => ({
  items: [],
  loading: true,
  error: null,
  query: "",
  view: "grid",
  filter: "all",
  report: null,
  progress: null,
  running: {},
  load: async () => {
    set({ loading: true, error: null, progress: null });
    try {
      set({ items: await backend.list() });
    } catch (error) {
      set({ error: message(error) });
    } finally {
      set({ loading: false });
    }
  },
  scan: async () => {
    set({ loading: true, error: null });
    try {
      const report = await backend.scan();
      set({ report });
      await get().load();
    } catch (error) {
      set({ error: message(error), loading: false });
    }
  },
  setQuery: (query) => set({ query }),
  setView: (view) => set({ view }),
  setFilter: (filter) => set({ filter }),
  favorite: async (i) => {
    try {
      await backend.favorite(i.id, !i.favorite);
      await get().load();
    } catch (error) {
      set({ error: message(error) });
    }
  },
  launch: async (i) => {
    try {
      await backend.launch(i.id);
      await get().refreshRunning();
    } catch (error) {
      set({ error: message(error) });
    }
  },
  hide: async (i) => {
    try {
      await backend.hide(i.id, !i.hidden);
      await get().load();
    } catch (error) {
      set({ error: message(error) });
    }
  },
  remove: async (i) => {
    try {
      await backend.delete(i.id);
      await get().load();
    } catch (error) {
      set({ error: message(error) });
    }
  },
  refreshRunning: async () => {
    try {
      set({ running: await backend.running() });
    } catch {
      /* browser preview */
    }
  },
  setProgress: (progress) => set({ progress }),
}));
