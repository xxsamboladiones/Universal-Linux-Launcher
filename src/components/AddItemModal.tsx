import { useEffect, useState, type FormEvent } from "react";
import { FolderOpen, X } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { backend } from "../services/backend";
import type { CompatibilityConfig, ItemKind, LibraryItem, RuntimeInfo } from "../types/library";

interface Props {
  onClose: () => void;
  onSaved: () => Promise<void>;
  item?: LibraryItem;
}
const parseEnvironment = (text: string): Record<string, string> =>
  Object.fromEntries(
    text
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const index = line.indexOf("=");
        if (index < 1) throw new Error(`Variável inválida: ${line}`);
        return [line.slice(0, index).trim(), line.slice(index + 1)];
      }),
  );

export function AddItemModal({ onClose, onSaved, item }: Props) {
  const emptyCompatibility: CompatibilityConfig = { runtimeId: null, prefixPath: null, steamOverlay: false, gamemode: false, mangohud: false, gamescope: { enabled: false, width: null, height: null, outputWidth: null, outputHeight: null, fps: null, fullscreen: false, upscaler: null }, dxvk: false, vkd3d: false };
  const [name, setName] = useState(item?.name ?? "");
  const [kind, setKind] = useState<ItemKind>(item?.kind ?? "application");
  const [executable, setExecutable] = useState(item?.executable ?? "");
  const [argumentsText, setArgumentsText] = useState(
    item?.arguments.join("\n") ?? "",
  );
  const [workingDirectory, setWorkingDirectory] = useState(
    item?.workingDirectory ?? "",
  );
  const [environmentText, setEnvironmentText] = useState(
    item
      ? Object.entries(item.environment)
          .map(([key, value]) => `${key}=${value}`)
          .join("\n")
      : "",
  );
  const [category, setCategory] = useState(item?.category ?? "");
  const [terminal, setTerminal] = useState(item?.terminal ?? false);
  const [compatibility, setCompatibility] = useState<CompatibilityConfig>(item?.compatibility ?? emptyCompatibility);
  const [runtimes, setRuntimes] = useState<RuntimeInfo[]>([]);
  useEffect(() => { void backend.compatibility().then((value) => setRuntimes(value.runtimes)).catch(() => undefined); }, []);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setError(null);
    if (!name.trim() || !executable.trim()) {
      setError("Nome e executável são obrigatórios.");
      return;
    }
    setSaving(true);
    try {
      await backend.save({
        id: item?.id,
        name: name.trim(),
        kind,
        provider: item?.provider ?? "custom",
        executable: executable.trim(),
        arguments: argumentsText
          .split("\n")
          .map((value) => value.trim())
          .filter(Boolean),
        workingDirectory: workingDirectory.trim() || null,
        environment: parseEnvironment(environmentText),
        icon: item?.icon ?? null,
        category: category.trim() || null,
        terminal,
        compatibility,
      });
      await onSaved();
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSaving(false);
    }
  };
  const chooseExecutable = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      title: "Selecionar executável",
    });
    if (selected) setExecutable(selected);
  };
  const chooseDirectory = async () => {
    const selected = await open({
      multiple: false,
      directory: true,
      title: "Selecionar diretório de trabalho",
    });
    if (selected) setWorkingDirectory(selected);
  };
  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <form
        className="modal"
        aria-modal="true"
        onSubmit={(event) => void submit(event)}
      >
        <div className="modal-title">
          <div>
            <p>{item ? "EDITAR ITEM" : "NOVO ITEM"}</p>
            <h2>{item ? item.name : "Adicionar aplicativo"}</h2>
          </div>
          <button aria-label="Fechar" type="button" onClick={onClose}>
            <X />
          </button>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="form-grid">
          <label>
            Nome
            <input
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label>
            Tipo
            <select
              value={kind}
              onChange={(event) => setKind(event.target.value as ItemKind)}
            >
              <option value="application">Aplicativo</option>
              <option value="game">Jogo</option>
              <option value="script">Script</option>
              <option value="custom">Personalizado</option>
            </select>
          </label>
          <label className="wide">
            Executável
            <div className="path-input">
              <input
                value={executable}
                onChange={(event) => setExecutable(event.target.value)}
                placeholder="/usr/bin/programa"
              />
              <button
                type="button"
                title="Procurar executável"
                onClick={() => void chooseExecutable()}
              >
                <FolderOpen size={18} />
                <span>Procurar</span>
              </button>
            </div>
          </label>
          <label className="wide">
            Argumentos <small>um argumento por linha</small>
            <textarea
              value={argumentsText}
              onChange={(event) => setArgumentsText(event.target.value)}
            />
          </label>
          <label className="wide">
            Diretório de trabalho
            <div className="path-input">
              <input
                value={workingDirectory}
                onChange={(event) => setWorkingDirectory(event.target.value)}
              />
              <button
                type="button"
                title="Selecionar pasta"
                onClick={() => void chooseDirectory()}
              >
                <FolderOpen size={18} />
                <span>Procurar</span>
              </button>
            </div>
          </label>
          <label>
            Categoria
            <input
              value={category}
              onChange={(event) => setCategory(event.target.value)}
            />
          </label>
          <label className="check wide">
            <input
              type="checkbox"
              checked={terminal}
              onChange={(event) => setTerminal(event.target.checked)}
            />
            Executar em terminal
          </label>
          <fieldset className="wide compatibility-fields">
            <legend>Compatibilidade CachyOS</legend>
            <label>Runtime
              <select value={compatibility.runtimeId ?? ""} onChange={(event) => setCompatibility({...compatibility, runtimeId:event.target.value || null})}>
                <option value="">Nativo / automático</option>
                {runtimes.map((runtime) => <option key={runtime.id} value={runtime.id}>{runtime.name} ({runtime.family})</option>)}
              </select>
            </label>
            <label className="wide">Prefixo Wine/Proton
              <div className="path-input"><input value={compatibility.prefixPath ?? ""} onChange={(event) => setCompatibility({...compatibility,prefixPath:event.target.value || null})}/><button type="button" onClick={() => void (async () => { const path=await backend.createPrefix(item?.id ?? name); setCompatibility({...compatibility,prefixPath:path}); })()}><FolderOpen size={18}/><span>Criar gerenciado</span></button></div>
            </label>
            <div className="compat-checks">
              <label className="check"><input type="checkbox" checked={compatibility.steamOverlay} onChange={(e)=>setCompatibility({...compatibility,steamOverlay:e.target.checked})}/>Steam Overlay</label>
              {([['gamemode','GameMode'],['mangohud','MangoHud'],['dxvk','DXVK'],['vkd3d','VKD3D']] as const).map(([key,label]) => <label className="check" key={key}><input type="checkbox" checked={compatibility[key]} onChange={(e)=>setCompatibility({...compatibility,[key]:e.target.checked})}/>{label}</label>)}
              <label className="check"><input type="checkbox" checked={compatibility.gamescope.enabled} onChange={(e)=>setCompatibility({...compatibility,gamescope:{...compatibility.gamescope,enabled:e.target.checked}})}/>Gamescope</label>
            </div>
            {compatibility.steamOverlay && <small className="steam-overlay-note">A Steam será iniciada em segundo plano e o Overlay será carregado diretamente no Proton selecionado pelo Orbit. Não exige atalho não-Steam.</small>}
            {compatibility.gamescope.enabled && <div className="gamescope-grid"><label>Largura<input type="number" value={compatibility.gamescope.width ?? ''} onChange={(e)=>setCompatibility({...compatibility,gamescope:{...compatibility.gamescope,width:e.target.value ? Number(e.target.value):null}})}/></label><label>Altura<input type="number" value={compatibility.gamescope.height ?? ''} onChange={(e)=>setCompatibility({...compatibility,gamescope:{...compatibility.gamescope,height:e.target.value ? Number(e.target.value):null}})}/></label><label>FPS<input type="number" value={compatibility.gamescope.fps ?? ''} onChange={(e)=>setCompatibility({...compatibility,gamescope:{...compatibility.gamescope,fps:e.target.value ? Number(e.target.value):null}})}/></label></div>}
            {item && <button type="button" className="icon" onClick={() => void backend.openCompatibilityLog(item.id)}>Abrir log de compatibilidade</button>}
          </fieldset>
          <label className="wide">
            Variáveis de ambiente <small>KEY=VALUE, uma por linha</small>
            <textarea
              value={environmentText}
              onChange={(event) => setEnvironmentText(event.target.value)}
            />
          </label>
        </div>
        <div className="modal-actions">
          <button type="button" onClick={onClose}>
            Cancelar
          </button>
          <button className="primary" disabled={saving}>
            {saving
              ? "Salvando…"
              : item
                ? "Salvar alterações"
                : "Adicionar à biblioteca"}
          </button>
        </div>
      </form>
    </div>
  );
}
