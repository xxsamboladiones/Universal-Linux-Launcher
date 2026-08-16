import { create } from "zustand";
import { backend } from "../services/backend";
import { useLibrary } from "./library";
import type {
  PlatformOverview,
  StoreId,
  TransferOperation,
} from "../types/platform";

interface PlatformState {
  overview: PlatformOverview | null;
  loading: boolean;
  preparing: Partial<Record<StoreId, boolean>>;
  syncing: Partial<Record<StoreId, boolean>>;
  removing: Record<string, boolean>;
  error: string | null;
  notice: string | null;
  load: () => Promise<void>;
  prepare: (provider: StoreId) => Promise<void>;
  connect: (provider: StoreId, user?: string) => Promise<void>;
  syncLibrary: (provider: StoreId) => Promise<void>;
  retry: (id: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  applyOperationProgress: (operation: TransferOperation) => void;
}

export const usePlatform = create<PlatformState>((set, get) => ({
  overview: null,
  loading: false,
  preparing: {},
  syncing: {},
  removing: {},
  error: null,
  notice: null,
  load: async () => {
    set({ loading: true, error: null, notice: null });
    try {
      set({ overview: await backend.platformOverview(), loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
    }
  },
  prepare: async (provider) => {
    if (get().preparing[provider]) return;
    set((state) => ({
      preparing: { ...state.preparing, [provider]: true },
      error: null,
      notice: null,
    }));
    try {
      await backend.prepareProvider(provider);
      await get().load();
      set((state) => {
        const preparing = { ...state.preparing };
        delete preparing[provider];
        return { preparing };
      });
    } catch (error) {
      set((state) => {
        const preparing = { ...state.preparing };
        delete preparing[provider];
        return { error: String(error), preparing };
      });
    }
  },
  connect: async (provider, user) => {
    set({ loading: true, error: null, notice: null });
    try {
      await backend.connectProvider(provider, user);
      await get().load();
    } catch (error) {
      set({ error: String(error), loading: false });
    }
  },
  syncLibrary: async (provider) => {
    if (get().syncing[provider]) return;
    set((state) => ({
      syncing: { ...state.syncing, [provider]: true },
      error: null,
      notice: null,
    }));
    try {
      const count = await backend.syncStoreLibrary(provider);
      const [overview, items] = await Promise.all([
        backend.platformOverview(),
        backend.list(),
      ]);
      useLibrary.setState({ items, error: null });
      set({
        overview,
        notice: `${count} ${count === 1 ? "jogo sincronizado" : "jogos sincronizados"} ${provider === "epic" ? "da Epic Games" : "do GOG"}.`,
      });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : String(error),
        notice: null,
      });
    } finally {
      set((state) => {
        const syncing = { ...state.syncing };
        delete syncing[provider];
        return { syncing };
      });
    }
  },
  retry: async (id) => {
    set({ error: null, notice: null });
    try { await backend.retryOperation(id); await get().load(); }
    catch (error) { set({ error: String(error) }); }
  },
  cancel: async (id) => {
    const operation = get().overview?.operations.find(
      (current) => current.id === id,
    );
    if (!operation || operation.state !== "running") return;

    set((state) => ({
      error: null,
      notice: null,
      overview: state.overview
        ? {
            ...state.overview,
            operations: state.overview.operations.map((current) =>
              current.id === id
                ? { ...current, state: "cancelling" as const }
                : current,
            ),
          }
        : null,
    }));
    try {
      await backend.cancelStoreOperation(id);
      await get().load();
    } catch (error) {
      await get().load();
      set({ error: String(error) });
    }
  },
  remove: async (id) => {
    if (get().removing[id]) return;
    set((state) => ({
      error: null,
      notice: null,
      removing: { ...state.removing, [id]: true },
    }));
    try {
      await backend.removeStoreOperation(id);
      set((state) => {
        const removing = { ...state.removing };
        delete removing[id];
        return {
          removing,
          overview: state.overview
            ? {
                ...state.overview,
                operations: state.overview.operations.filter(
                  (operation) => operation.id !== id,
                ),
              }
            : null,
        };
      });
    } catch (error) {
      set((state) => {
        const removing = { ...state.removing };
        delete removing[id];
        return { error: String(error), removing };
      });
    }
  },
  applyOperationProgress: (operation) => {
    set((state) => {
      if (!state.overview) return state;

      const operationExists = state.overview.operations.some(
        (current) => current.id === operation.id,
      );
      const operations = operationExists
        ? state.overview.operations.map((current) =>
            current.id === operation.id ? operation : current,
          )
        : [operation, ...state.overview.operations];

      return {
        overview: {
          ...state.overview,
          operations,
        },
      };
    });
  },
}));
