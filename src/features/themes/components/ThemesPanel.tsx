import { LoaderCircle, Upload } from "lucide-react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useThemeStore } from "../stores/theme";
import { AutomaticThemePanel } from "./AutomaticThemePanel";
import { ThemeCard } from "./ThemeCard";

export function ThemesPanel() {
  const themes = useThemeStore((state) => state.themes);
  const active = useThemeStore((state) => state.active);
  const settings = useThemeStore((state) => state.settings);
  const status = useThemeStore((state) => state.status);
  const error = useThemeStore((state) => state.error);
  const loaded = useThemeStore((state) => state.loaded);
  const selectManualTheme = useThemeStore((state) => state.selectManualTheme);
  const importFile = useThemeStore((state) => state.importFile);
  const remove = useThemeStore((state) => state.remove);
  const exportFile = useThemeStore((state) => state.exportFile);
  const busy = status !== "idle";

  const importTheme = async () => {
    const path = await open({
      title: "Importar tema do Orbit",
      multiple: false,
      directory: false,
      filters: [{ name: "Tema Orbit", extensions: ["orbit-theme"] }],
    });
    if (typeof path === "string") await importFile(path);
  };

  const exportTheme = async (id: string) => {
    const path = await save({
      title: "Exportar tema",
      defaultPath: `${id}.orbit-theme`,
      filters: [{ name: "Tema Orbit", extensions: ["orbit-theme"] }],
    });
    if (path) await exportFile(id, path);
  };

  return (
    <section className="settings-panel themes-panel" aria-busy={busy}>
      <div className="panel-heading">
        <div>
          <h2>Temas</h2>
          <p>
            Escolha um visual oficial ou instale um tema declarativo validado.
          </p>
        </div>
        <button
          className="theme-import"
          disabled={busy}
          onClick={() => void importTheme()}
        >
          <Upload size={15} />
          {status === "importing" ? "Importando…" : "Importar tema"}
        </button>
      </div>
      {error && (
        <p className="theme-error" role="alert">
          {error}
        </p>
      )}
      <AutomaticThemePanel />
      {!loaded ? (
        <p className="theme-loading">
          <LoaderCircle size={16} /> Carregando temas…
        </p>
      ) : (
        <div className="theme-grid">
          {themes.map((theme) => (
            <ThemeCard
              key={theme.id}
              theme={theme}
              selected={
                settings.themeMode === "manual" && active?.id === theme.id
              }
              busy={busy}
              onApply={() => void selectManualTheme(theme.id)}
              onExport={() => void exportTheme(theme.id)}
              onRemove={() => {
                if (window.confirm(`Remover o tema ${theme.name}?`)) {
                  void remove(theme.id);
                }
              }}
            />
          ))}
        </div>
      )}
    </section>
  );
}
