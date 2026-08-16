import { Download, LoaderCircle, Palette, Trash2, Upload } from "lucide-react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useThemeStore } from "../stores/theme";

export function ThemesPanel() {
  const { themes, active, status, error, loaded, apply, importFile, remove, exportFile } = useThemeStore();
  const busy = status !== "idle";
  const importTheme = async () => {
    const path = await open({ title: "Importar tema do Orbit", multiple: false, directory: false, filters: [{ name: "Tema Orbit", extensions: ["orbit-theme"] }] });
    if (typeof path === "string") await importFile(path);
  };
  const exportTheme = async (id: string) => {
    const path = await save({ title: "Exportar tema", defaultPath: `${id}.orbit-theme`, filters: [{ name: "Tema Orbit", extensions: ["orbit-theme"] }] });
    if (path) await exportFile(id, path);
  };
  return <section className="settings-panel themes-panel" aria-busy={busy}>
    <div className="panel-heading">
      <div><h2>Temas</h2><p>Escolha um visual oficial ou instale um tema declarativo validado.</p></div>
      <button className="theme-import" disabled={busy} onClick={() => void importTheme()}><Upload size={15}/> {status === "importing" ? "Importando…" : "Importar tema"}</button>
    </div>
    {error && <p className="theme-error" role="alert">{error}</p>}
    {!loaded ? <p className="theme-loading"><LoaderCircle size={16}/> Carregando temas…</p> : <div className="theme-grid">
      {themes.map((theme) => {
        const selected = active?.id === theme.id;
        return <article className={`theme-card ${selected ? "active" : ""}`} key={theme.id}>
          <div className={`theme-preview ${theme.type}`} style={{ background: theme.previewUrl ? undefined : `linear-gradient(145deg, var(--orbit-color-primary), var(--orbit-color-surface))` }}>
            {theme.previewUrl ? <img src={theme.previewUrl} alt={`Prévia do tema ${theme.name}`} loading="lazy" /> : <Palette size={30} aria-hidden="true" />}
          </div>
          <div className="theme-card-copy"><div><strong>{theme.name}</strong><small>por {theme.author} · v{theme.version}</small></div><span className={`theme-type ${theme.type}`}>{theme.type === "dark" ? "Escuro" : "Claro"}</span></div>
          <p>{theme.description}</p>
          <div className="theme-actions">
            <button className={selected ? "selected" : ""} disabled={busy || selected || !theme.compatible} onClick={() => void apply(theme.id)}>{selected ? "✓ Ativo" : "Aplicar"}</button>
            {theme.source === "external" && <><button title="Exportar tema" disabled={busy} onClick={() => void exportTheme(theme.id)}><Download size={15}/></button><button className="danger" title="Remover tema" disabled={busy} onClick={() => { if (window.confirm(`Remover o tema ${theme.name}?`)) void remove(theme.id); }}><Trash2 size={15}/></button></>}
          </div>
          <small className="theme-origin">{theme.source === "builtin" ? "Interno" : "Externo"} · Orbit {theme.orbitVersion}</small>
        </article>;
      })}
    </div>}
  </section>;
}
