import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppSettings } from "../../../types/library";
import { useThemeStore } from "../stores/theme";

export function AutomaticThemePanel() {
  const settings = useThemeStore((state) => state.settings);
  const automatic = useThemeStore((state) => state.automatic);
  const pywal = useThemeStore((state) => state.pywal);
  const status = useThemeStore((state) => state.status);
  const updateSettings = useThemeStore((state) => state.updateSettings);
  const setThemeMode = useThemeStore((state) => state.setThemeMode);
  const setPaletteSource = useThemeStore((state) => state.setPaletteSource);
  const refreshAutomatic = useThemeStore((state) => state.refreshAutomatic);
  const busy = status !== "idle";
  const [influence, setInfluence] = useState(settings.wallpaperInfluence);

  useEffect(
    () => setInfluence(settings.wallpaperInfluence),
    [settings.wallpaperInfluence],
  );

  const patchSettings = <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) => void updateSettings({ ...settings, [key]: value });

  const chooseWallpaper = async () => {
    const path = await open({
      title: "Selecionar wallpaper",
      multiple: false,
      directory: false,
      filters: [
        { name: "Imagens", extensions: ["png", "jpg", "jpeg", "webp"] },
      ],
    });
    if (typeof path === "string") {
      await updateSettings({ ...settings, manualWallpaperPath: path });
      if (settings.themeMode === "automatic") await refreshAutomatic();
    }
  };

  const commitInfluence = async () => {
    if (influence === settings.wallpaperInfluence) return;
    await updateSettings({ ...settings, wallpaperInfluence: influence });
    await refreshAutomatic();
  };

  const setColorMode = async (
    automaticColorMode: AppSettings["automaticColorMode"],
  ) => {
    await updateSettings({ ...settings, automaticColorMode });
    await refreshAutomatic();
  };

  return (
    <section className="automatic-theme" aria-busy={status === "generating"}>
      <div>
        <strong>Tema automático</strong>
        <small>
          Pywal é opcional; o Orbit usa seu gerador nativo quando necessário.
        </small>
      </div>
      <label>
        Modo
        <select
          value={settings.themeMode}
          disabled={busy}
          onChange={(event) =>
            void setThemeMode(event.target.value as AppSettings["themeMode"])
          }
        >
          <option value="manual">Manual</option>
          <option value="automatic">Automático</option>
          <option value="system">Sistema</option>
        </select>
      </label>
      {settings.themeMode === "automatic" && (
        <>
          <label>
            Fonte
            <select
              value={settings.paletteSource}
              disabled={busy}
              onChange={(event) =>
                void setPaletteSource(
                  event.target.value as AppSettings["paletteSource"],
                )
              }
            >
              <option value="automatic">Automática</option>
              <option value="pywal" disabled={!pywal?.available}>
                Pywal
                {pywal?.available ? ` (${pywal.provider})` : " (indisponível)"}
              </option>
              <option value="native">Orbit Native</option>
            </select>
          </label>
          <label>
            Modo de cor
            <select
              value={settings.automaticColorMode}
              disabled={busy}
              onChange={(event) =>
                void setColorMode(
                  event.target.value as AppSettings["automaticColorMode"],
                )
              }
            >
              <option value="automatic">Automático</option>
              <option value="dark">Escuro</option>
              <option value="light">Claro</option>
            </select>
          </label>
          <label>
            Influência
            <input
              type="range"
              min="0"
              max="100"
              value={influence}
              disabled={busy}
              onChange={(event) => setInfluence(Number(event.target.value))}
              onPointerUp={() => void commitInfluence()}
              onKeyUp={() => void commitInfluence()}
              onBlur={() => void commitInfluence()}
            />
            <output>{influence}%</output>
          </label>
          {settings.paletteSource === "pywal" ? (
            <small className="automatic-update-note">
              Atualização automática ativa: o Orbit acompanha as alterações em
              <code> colors.json</code>.
            </small>
          ) : (
            <label>
              <span>Atualizar ao mudar wallpaper</span>
              <input
                type="checkbox"
                checked={settings.automaticUpdate}
                disabled={busy}
                onChange={(event) =>
                  patchSettings("automaticUpdate", event.target.checked)
                }
              />
            </label>
          )}
          <button
            className="theme-import"
            disabled={busy}
            onClick={() => void chooseWallpaper()}
          >
            Escolher wallpaper
          </button>
          <button
            className="theme-import"
            disabled={busy}
            onClick={() => void refreshAutomatic()}
          >
            {status === "generating" ? "Gerando…" : "Regenerar agora"}
          </button>
          {settings.manualWallpaperPath && (
            <small>
              Wallpaper manual: {settings.manualWallpaperPath.split("/").pop()}
            </small>
          )}
          {automatic && (
            <small>
              Fonte: {automatic.source} ·{" "}
              {automatic.wallpaperPath.split("/").pop()}
            </small>
          )}
        </>
      )}
    </section>
  );
}
