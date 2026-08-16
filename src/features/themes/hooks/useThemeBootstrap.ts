import { useEffect } from "react";
import { useThemeStore } from "../stores/theme";
export function useThemeBootstrap() { const load = useThemeStore((state) => state.load); useEffect(() => { void load(); }, [load]); }
