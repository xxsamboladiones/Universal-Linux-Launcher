import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { AutomaticTheme } from "../types";
import { useThemeStore } from "../stores/theme";
export function useThemeBootstrap() {
  const load = useThemeStore((state) => state.load);
  const applyAutomatic = useThemeStore((state) => state.applyAutomatic);
  const themeMode = useThemeStore((state) => state.settings.themeMode);
  const setThemeMode = useThemeStore((state) => state.setThemeMode);

  useEffect(() => {
    void load();
    const unlisten = listen<AutomaticTheme>(
      "automatic-theme-updated",
      (event) => applyAutomatic(event.payload),
    );
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [applyAutomatic, load]);

  useEffect(() => {
    if (themeMode !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const update = () => void setThemeMode("system");
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [setThemeMode, themeMode]);
}
