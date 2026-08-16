import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { AutomaticTheme } from "../types";
import { useThemeStore } from "../stores/theme";
export function useThemeBootstrap() { const load = useThemeStore((state) => state.load); const applyAutomatic = useThemeStore((state) => state.applyAutomatic); useEffect(() => { void load(); const unlisten = listen<AutomaticTheme>("automatic-theme-updated", (event) => applyAutomatic(event.payload)); return () => { void unlisten.then((fn) => fn()); }; }, [applyAutomatic, load]); }
