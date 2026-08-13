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
import type { AppSettings } from "./types/library";
import { backend } from "./services/backend";
import "./styles.css";
import "./platform.css";
import "./modal.css";
import "./settings.css";
import "./phase1.css";
import "./general-settings.css";
import "./file-picker.css";
import "./phase2.css";
import "./controls.css";
export default function App() {
  const s = useLibrary();
  const { load, scan, setFilter, refreshRunning, setProgress } = s;
  const [adding, setAdding] = useState(false);
  const [restoring, setRestoring] = useState<string | null>(null);
  const [editing, setEditing] = useState<LibraryItem | null>(null);
  const [settings, setSettings] = useState<AppSettings>({
    theme: "dark",
    scanOnStartup: false,
    confirmBeforeRemove: true,
    preferredTerminal: "konsole",
  });
  const input = useRef<HTMLInputElement>(null);
  const startupConfigured = useRef(false);
  useEffect(() => {
    if (startupConfigured.current) return;
    startupConfigured.current = true;
    void load();
    void backend.settings().then((value) => {
      setSettings(value);
      document.documentElement.dataset.theme = value.theme;
      if (value.scanOnStartup) void scan();
    });
  }, [load, scan]);
  const saveSettings = async (value: AppSettings) => {
    setSettings(value);
    document.documentElement.dataset.theme = value.theme;
    await backend.updateSettings(value);
  };
  useEffect(() => {
    const unlisten = listen<ScanProgress>("scan-progress", (event) =>
      setProgress(event.payload),
    );
    void refreshRunning();
    const timer = setInterval(() => void refreshRunning(), 2000);
    return () => {
      clearInterval(timer);
      void unlisten.then((fn) => fn());
    };
  }, [refreshRunning, setProgress]);
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
  const items = useMemo(
    () =>
      s.items
        .filter(
          (i) =>
            !i.hidden &&
            i.installed &&
            (s.filter === "all" ||
              s.filter === "home" ||
              (s.filter === "favorites" && i.favorite) ||
              i.kind === s.filter ||
              i.provider === s.filter),
        )
        .filter((i) =>
          `${i.name} ${i.provider} ${i.category ?? ""} ${i.tags.join(" ")}`
            .toLowerCase()
            .includes(s.query.toLowerCase()),
        ),
    [s.items, s.filter, s.query],
  );
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
              <p>SUA BIBLIOTECA</p>
              <h1>
                {s.filter === "home" ? "Olá, pronto para jogar?" : "Biblioteca"}
              </h1>
              <span>{items.length} itens disponíveis no seu universo</span>
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
                  onLaunch={() => void s.launch(i)}
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
                  Atualize a biblioteca para encontrar Steam e aplicativos do
                  sistema.
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
