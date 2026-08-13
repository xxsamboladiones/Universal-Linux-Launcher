import { create } from "zustand";
import { backend } from "../services/backend";
import type { PlatformOverview, StoreId } from "../types/platform";

interface PlatformState {
  overview: PlatformOverview | null;
  loading: boolean;
  preparing: Partial<Record<StoreId, boolean>>;
  error: string | null;
  load: () => Promise<void>;
  prepare: (provider: StoreId) => Promise<void>;
  connect: (provider: StoreId) => Promise<void>;
  retry: (id: string) => Promise<void>;
}

export const usePlatform = create<PlatformState>((set, get) => ({
  overview: null,
  loading: false,
  preparing: {},
  error: null,
  load: async () => {
    set({ loading: true, error: null });
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
  connect: async (provider) => {
    set({ loading: true, error: null });
    try { await backend.connectProvider(provider); set({ loading: false }); }
    catch (error) { set({ error: String(error), loading: false }); }
  },
  retry: async (id) => {
    set({ error: null });
    try { await backend.retryOperation(id); await get().load(); }
    catch (error) { set({ error: String(error) }); }
  },
}));
