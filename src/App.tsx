import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Grid2X2, List, Plus, RefreshCw, Search } from "lucide-react";
import { Sidebar } from "./components/Sidebar";
import { ItemCard } from "./components/ItemCard";
import { AddItemModal } from "./components/AddItemModal";
import { PlatformsPage } from "./components/PlatformsPage";
import { SettingsPage } from "./components/SettingsPage";
import { useLibrary } from "./stores/library";
import type { LibraryItem } from "./types/library";
import type { ScanProgress } from "./types/library";
import type { TransferOperation } from "./types/platform";
import { visibleInLibrary } from "./services/library-filter";
import { useThemeBootstrap } from "./features/themes/hooks/useThemeBootstrap";
import { useThemeStore } from "./features/themes/stores/theme";
import "./styles.css";
import "./platform.css";
import "./modal.css";
import "./settings.css";
import "./phase1.css";
import "./general-settings.css";
import "./file-picker.css";
import "./phase2.css";
import "./controls.css";
import "./features/themes/theme.css";
export default function App() {
  useThemeBootstrap();
  const s = useLibrary();
  const { load, scan, setFilter, refreshRunning, setProgress, applyTransfer } =
    s;
  const [adding, setAdding] = useState(false);
  const [restoring, setRestoring] = useState<string | null>(null);
  const [editing, setEditing] = useState<LibraryItem | null>(null);
  const settings = useThemeStore((state) => state.settings);
  const settingsLoaded = useThemeStore((state) => state.loaded);
  const saveSettings = useThemeStore((state) => state.updateSettings);
  const input = useRef<HTMLInputElement>(null);
  const startupScanHandled = useRef(false);
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    if (!settingsLoaded || startupScanHandled.current) return;
    startupScanHandled.current = true;
    if (settings.scanOnStartup) void scan();
  }, [scan, settings.scanOnStartup, settingsLoaded]);
  useEffect(() => {
    const unlisten = listen<ScanProgress>("scan-progress", (event) =>
      setProgress(event.payload),
    );
    const unlistenTransfer = listen<TransferOperation>(
      "transfer-progress",
      (event) => applyTransfer(event.payload),
    );
    const unlistenLibrary = listen<string>(
      "library-changed",
      () => void load(),
    );
    void refreshRunning();
    const timer = setInterval(() => void refreshRunning(), 2000);
    return () => {
      clearInterval(timer);
      void unlisten.then((fn) => fn());
      void unlistenTransfer.then((fn) => fn());
      void unlistenLibrary.then((fn) => fn());
    };
  }, [applyTransfer, load, refreshRunning, setProgress]);
  useEffect(() => {
    const fn = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        input.current?.focus();
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        void scan();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === ",") {
        e.preventDefault();
        setFilter("settings");
      }
      if (e.key === "Escape") setAdding(false);
    };
    addEventListener("keydown", fn);
    return () => removeEventListener("keydown", fn);
  }, [scan, setFilter]);
  const filteredItems = useMemo(
    () => s.items.filter((item) => visibleInLibrary(item, s.filter)),
    [s.items, s.filter],
  );
  const storeCatalog = ["epic", "gog"].includes(s.filter);
  const storeName = s.filter === "gog" ? "GOG" : "Epic Games";
  const items = useMemo(
    () =>
      filteredItems
        .filter((i) =>
          `${i.name} ${i.provider} ${i.category ?? ""} ${i.tags.join(" ")}`
            .toLowerCase()
            .includes(s.query.toLowerCase()),
        )
        .sort((left, right) =>
          ["epic", "gog"].includes(s.filter)
            ? Number(right.installed) - Number(left.installed) ||
              left.name.localeCompare(right.name, "pt-BR")
            : 0,
        ),
    [filteredItems, s.filter, s.query],
  );
  const storeInstalled = storeCatalog
    ? filteredItems.filter((item) => item.installed).length
    : 0;
  const restore = async (item: LibraryItem) => {
    setRestoring(item.id);
    try {
      await s.hide(item);
    } finally {
      setRestoring(null);
    }
  };
  return (
    <main>
      <Sidebar active={s.filter} onChange={s.setFilter} />
      <section className="content">
        <header>
          <div className="search">
            <Search size={19} />
            <input
              ref={input}
              value={s.query}
              onChange={(e) => s.setQuery(e.target.value)}
              placeholder="Buscar jogos e aplicativos..."
            />
            <kbd>Ctrl K</kbd>
          </div>
          <button
            className="icon"
            disabled={s.loading}
            onClick={() => void scan()}
            title="Atualizar biblioteca"
          >
            <RefreshCw className={s.loading ? "spin" : ""} size={19} />
          </button>
          <button className="primary" onClick={() => setAdding(true)}>
            <Plus size={18} />
            Adicionar
          </button>
        </header>
        {s.error && <div className="global-error">{s.error}</div>}
        {s.filter === "platforms" ? (
          <PlatformsPage />
        ) : s.filter === "settings" ? (
          <SettingsPage
            items={s.items}
            restoring={restoring}
            onRestore={restore}
            settings={settings}
            onSettings={saveSettings}
          />
        ) : (
          <>
            <div className="hero">
              <p>
                {storeCatalog
                  ? `SUA CONTA ${s.filter.toUpperCase()}`
                  : "SUA BIBLIOTECA"}
              </p>
              <h1>
                {s.filter === "home"
                  ? "Olá, pronto para jogar?"
                  : storeCatalog
                    ? storeName
                    : "Biblioteca"}
              </h1>
              <span>
                {storeCatalog
                  ? `${filteredItems.length} jogos na conta · ${storeInstalled} instalados`
                  : `${items.length} itens disponíveis no seu universo`}
              </span>
            </div>
            <div className="toolbar">
              <div>
                <button
                  className={s.view === "grid" ? "selected" : ""}
                  onClick={() => s.setView("grid")}
                >
                  <Grid2X2 size={17} />
                </button>
                <button
                  className={s.view === "list" ? "selected" : ""}
                  onClick={() => s.setView("list")}
                >
                  <List size={18} />
                </button>
              </div>
              <span>
                {s.loading
                  ? s.progress
                    ? `${s.progress.provider}: ${s.progress.status === "scanning" ? "escaneando" : `${s.progress.found} encontrados`}`
                    : "Escaneando…"
                  : `${items.length} itens`}
              </span>
            </div>
            {s.report && (
              <div className="report">
                Descoberta concluída: {s.report.added} novos, {s.report.updated}{" "}
                atualizados.
              </div>
            )}
            <div
              className={s.view === "grid" ? "library grid" : "library list"}
            >
              {items.map((i) => (
                <ItemCard
                  key={i.id}
                  item={i}
                  running={i.id in s.running}
                  installing={Boolean(s.installing[i.id])}
                  uninstalling={Boolean(s.uninstalling[i.id])}
                  onLaunch={() => void s.launch(i)}
                  onInstall={() => void s.install(i)}
                  onUninstall={() => {
                    const detail =
                      i.provider === "appimage"
                        ? "O arquivo AppImage será movido para a lixeira."
                        : "Os arquivos do programa serão removidos; saves e configurações externas serão preservados quando o provider permitir.";
                    if (confirm(`Desinstalar ${i.name}?\n\n${detail}`)) {
                      void s.uninstall(i);
                    }
                  }}
                  onFavorite={() => void s.favorite(i)}
                  onHide={() => void s.hide(i)}
                  onEdit={() => setEditing(i)}
                  onDelete={() => {
                    if (
                      !settings.confirmBeforeRemove ||
                      confirm(`Remover ${i.name} da biblioteca?`)
                    )
                      void s.remove(i);
                  }}
                />
              ))}
            </div>
            {!s.loading && !items.length && (
              <div className="empty">
                <Gamepad2 />
                <h2>Sua órbita está vazia</h2>
                <p>
                  {storeCatalog
                    ? `Conecte sua conta e sincronize a biblioteca ${s.filter === "epic" ? "da Epic Games" : "do GOG"}.`
                    : "Atualize a biblioteca para encontrar Steam e aplicativos do sistema."}
                </p>
              </div>
            )}
          </>
        )}
        {adding && (
          <AddItemModal onClose={() => setAdding(false)} onSaved={load} />
        )}
        {editing && (
          <AddItemModal
            item={editing}
            onClose={() => setEditing(null)}
            onSaved={load}
          />
        )}
      </section>
    </main>
  );
}
function Gamepad2() {
  return <span className="empty-icon">◎</span>;
}
