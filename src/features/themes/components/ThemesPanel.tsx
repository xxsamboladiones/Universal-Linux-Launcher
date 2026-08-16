import { Download, LoaderCircle, Palette, Trash2, Upload } from "lucide-react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useThemeStore } from "../stores/theme";
import type { AppSettings } from "../../../types/library";

interface Props { settings: AppSettings; onSettings(settings: AppSettings): Promise<void>; }

export function ThemesPanel({ settings, onSettings }: Props) {
  const { themes, active, automatic, pywal, status, error, loaded, apply, refreshAutomatic, importFile, remove, exportFile } = useThemeStore();
  const busy = status !== "idle";
  const importTheme = async () => {
    const path = await open({ title: "Importar tema do Orbit", multiple: false, directory: false, filters: [{ name: "Tema Orbit", extensions: ["orbit-theme"] }] });
    if (typeof path === "string") await importFile(path);
  };
  const exportTheme = async (id: string) => {
    const path = await save({ title: "Exportar tema", defaultPath: `${id}.orbit-theme`, filters: [{ name: "Tema Orbit", extensions: ["orbit-theme"] }] });
    if (path) await exportFile(id, path);
  };
  const chooseWallpaper = async () => {
    const path = await open({ title: "Selecionar wallpaper", multiple: false, directory: false, filters: [{ name: "Imagens", extensions: ["png", "jpg", "jpeg", "webp"] }] });
    if (typeof path === "string") await onSettings({ ...settings, manualWallpaperPath: path });
  };
  const setPaletteSource = (paletteSource: AppSettings["paletteSource"]) =>
    void onSettings({
      ...settings,
      paletteSource,
      // Pywal já é um fluxo reativo: ao escolhê-lo, o watcher deve ficar
      // ativo sem exigir uma segunda decisão redundante do usuário.
      automaticUpdate: paletteSource === "pywal" ? true : settings.automaticUpdate,
    });
  return <section className="settings-panel themes-panel" aria-busy={busy}>
    <div className="panel-heading">
      <div><h2>Temas</h2><p>Escolha um visual oficial ou instale um tema declarativo validado.</p></div>
      <button className="theme-import" disabled={busy} onClick={() => void importTheme()}><Upload size={15}/> {status === "importing" ? "Importando…" : "Importar tema"}</button>
    </div>
    {error && <p className="theme-error" role="alert">{error}</p>}
    <section className="automatic-theme" aria-busy={status === "generating"}>
      <div><strong>Tema automático</strong><small>Pywal é opcional; o Orbit usa seu gerador nativo quando necessário.</small></div>
      <label>Modo <select value={settings.themeMode} onChange={(e) => void onSettings({ ...settings, themeMode: e.target.value as AppSettings["themeMode"] })}><option value="manual">Manual</option><option value="automatic">Automático</option><option value="system">Sistema</option></select></label>
      {settings.themeMode === "automatic" && <><label>Fonte <select value={settings.paletteSource} onChange={(e) => setPaletteSource(e.target.value as AppSettings["paletteSource"])}><option value="automatic">Automática</option><option value="pywal" disabled={!pywal?.available}>Pywal{pywal?.available ? ` (${pywal.provider})` : " (indisponível)"}</option><option value="native">Orbit Native</option></select></label><label>Modo de cor <select value={settings.automaticColorMode} onChange={(e) => void onSettings({ ...settings, automaticColorMode: e.target.value as AppSettings["automaticColorMode"] })}><option value="automatic">Automático</option><option value="dark">Escuro</option><option value="light">Claro</option></select></label><label>Influência <input type="range" min="0" max="100" value={settings.wallpaperInfluence} onChange={(e) => void onSettings({ ...settings, wallpaperInfluence: Number(e.target.value) })}/>{settings.wallpaperInfluence}%</label>{settings.paletteSource === "pywal" ? <small>Atualização automática ativada pelo Pywal.</small> : <label><span>Atualizar ao mudar wallpaper</span><input type="checkbox" checked={settings.automaticUpdate} onChange={(e) => void onSettings({ ...settings, automaticUpdate: e.target.checked })}/></label>}<button className="theme-import" disabled={busy} onClick={() => void chooseWallpaper()}>Escolher wallpaper</button><button className="theme-import" disabled={busy} onClick={() => void refreshAutomatic()}>{status === "generating" ? "Gerando…" : "Gerar e visualizar"}</button>{settings.manualWallpaperPath && <small>Wallpaper manual: {settings.manualWallpaperPath.split("/").pop()}</small>}{automatic && <small>Fonte: {automatic.source} · {automatic.wallpaperPath.split("/").pop()}</small>}</>}
    </section>
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
