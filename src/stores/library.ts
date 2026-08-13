import { create } from "zustand";
import { backend } from "../services/backend";
import type { LibraryItem, ScanProgress, ScanReport } from "../types/library";
import type { TransferOperation } from "../types/platform";
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
  installing: Record<string, boolean>;
  uninstalling: Record<string, boolean>;
  load: () => Promise<void>;
  scan: () => Promise<void>;
  setQuery: (v: string) => void;
  setView: (v: "grid" | "list") => void;
  setFilter: (v: string) => void;
  favorite: (i: LibraryItem) => Promise<void>;
  launch: (i: LibraryItem) => Promise<void>;
  install: (i: LibraryItem) => Promise<boolean>;
  uninstall: (i: LibraryItem) => Promise<boolean>;
  hide: (i: LibraryItem) => Promise<void>;
  remove: (i: LibraryItem) => Promise<void>;
  refreshRunning: () => Promise<void>;
  setProgress: (progress: ScanProgress) => void;
  applyTransfer: (operation: TransferOperation) => void;
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
  installing: {},
  uninstalling: {},
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
  install: async (i) => {
    if (i.provider !== "epic" || i.installed || get().installing[i.id]) {
      return false;
    }
    set((state) => ({
      error: null,
      installing: { ...state.installing, [i.id]: true },
    }));
    try {
      await backend.queueStoreOperation(
        "epic",
        i.id.replace(/^epic:/, ""),
        "install",
      );
      return true;
    } catch (error) {
      set((state) => {
        const installing = { ...state.installing };
        delete installing[i.id];
        return { error: message(error), installing };
      });
      return false;
    }
  },
  uninstall: async (i) => {
    if (!i.installed || get().uninstalling[i.id] || i.id in get().running) {
      return false;
    }
    set((state) => ({
      error: null,
      uninstalling: { ...state.uninstalling, [i.id]: true },
    }));
    try {
      await backend.uninstall(i.id);
      await get().load();
      return true;
    } catch (error) {
      set({ error: message(error) });
      return false;
    } finally {
      set((state) => {
        const uninstalling = { ...state.uninstalling };
        delete uninstalling[i.id];
        return { uninstalling };
      });
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
  applyTransfer: (operation) => {
    if (operation.provider !== "epic" || operation.action !== "install") return;
    const itemId = `epic:${operation.itemId}`;
    const active = ["queued", "running", "cancelling", "paused"].includes(
      operation.state,
    );
    set((state) => {
      const installing = { ...state.installing };
      if (active) installing[itemId] = true;
      else delete installing[itemId];
      return {
        installing,
        error:
          operation.state === "failed" && operation.error
            ? operation.error
            : state.error,
      };
    });
  },
}));
