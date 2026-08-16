import { useEffect, useState } from "react";
import { Eye, EyeOff, Gauge, Library, RotateCcw, Settings2 } from "lucide-react";
import { backend } from "../services/backend";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { ProductStatus } from "../services/backend";
import type { UpdateStatus } from "../services/backend";
import type { AppSettings, CompatibilityOverview, LibraryItem } from "../types/library";
import { ThemesPanel } from "../features/themes/components/ThemesPanel";

interface Props {
  items: LibraryItem[];
  restoring: string | null;
  onRestore: (item: LibraryItem) => Promise<void>;
  settings: AppSettings;
  onSettings: (settings: AppSettings) => Promise<void>;
}
export function SettingsPage({
  items,
  restoring,
  onRestore,
  settings,
  onSettings,
}: Props) {
  const [section, setSection] = useState<"library" | "general" | "themes" | "compatibility">("library");
  const [compatibility, setCompatibility] = useState<CompatibilityOverview | null>(null);
  const [productStatus,setProductStatus]=useState<ProductStatus|null>(null);
  const [productMessage,setProductMessage]=useState<string|null>(null);
  const [updateStatus,setUpdateStatus]=useState<UpdateStatus|null>(null);
  useEffect(()=>{
    void backend.productStatus().then(setProductStatus);
    // A consulta é automática; a instalação continua exigindo confirmação do usuário.
    void backend.checkUpdates().then(setUpdateStatus).catch(() => undefined);
  },[]);
  useEffect(() => { if (section === "compatibility") void backend.compatibility().then(setCompatibility); }, [section]);
  const hidden = items.filter((item) => item.hidden && item.installed);
  const update = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) =>
    void onSettings({ ...settings, [key]: value });
  return (
    <div className="settings-page">
      <div className="hero compact">
        <p>CONFIGURAÇÕES</p>
        <h1>{section === "library" ? "Biblioteca" : section === "general" ? "Geral" : section === "themes" ? "Aparência" : "Compatibilidade"}</h1>
        <span>
          {section === "library"
            ? "Gerencie o conteúdo exibido pelo Orbit."
            : section === "general" ? "Preferências de comportamento." : section === "themes" ? "Personalize a aparência do Orbit." : "Runtimes e integração do sistema."}
        </span>
      </div>
      <div className="settings-layout">
        <nav className="settings-nav">
          <button
            className={section === "library" ? "active" : ""}
            onClick={() => setSection("library")}
          >
            <Library size={17} />
            Biblioteca
          </button>
          <button
            className={section === "general" ? "active" : ""}
            onClick={() => setSection("general")}
          >
            <Settings2 size={17} />
            Geral
          </button>
          <button className={section === "themes" ? "active" : ""} onClick={() => setSection("themes")}><PaletteIcon/>Aparência</button>
          <button className={section === "compatibility" ? "active" : ""} onClick={() => setSection("compatibility")}><Gauge size={17}/>Compatibilidade</button>
        </nav>
        {section === "library" ? (
          <section className="settings-panel">
            <div className="panel-heading">
              <div>
                <h2>Itens ocultos</h2>
                <p>
                  Itens ocultos continuam instalados e podem ser restaurados a
                  qualquer momento.
                </p>
              </div>
              <span>
                <EyeOff size={15} />
                {hidden.length}
              </span>
            </div>
            {hidden.length === 0 ? (
              <div className="hidden-empty">
                <Eye size={30} />
                <strong>Nenhum item oculto</strong>
                <p>Os jogos e aplicativos ocultados aparecerão aqui.</p>
              </div>
            ) : (
              <div className="hidden-list">
                {hidden.map((item) => (
                  <article key={item.id}>
                    <div className="hidden-icon">
                      {item.name.slice(0, 2).toUpperCase()}
                    </div>
                    <div>
                      <strong>{item.name}</strong>
                      <small>
                        {item.provider} · {item.category ?? item.kind}
                      </small>
                    </div>
                    <button
                      disabled={restoring === item.id}
                      onClick={() => void onRestore(item)}
                    >
                      <RotateCcw size={15} />
                      {restoring === item.id ? "Restaurando…" : "Restaurar"}
                    </button>
                  </article>
                ))}
              </div>
            )}
          </section>
        ) : section === "general" ? (
          <section className="settings-panel general-settings">
            <div className="panel-heading">
              <div>
                <h2>Preferências gerais</h2>
                <p>
                  As alterações são salvas automaticamente neste computador.
                </p>
              </div>
            </div>
            <label>
              <div>
                <strong>Tema</strong>
                <small>Acompanhar o sistema ou manter o visual escuro.</small>
              </div>
              <select
                value={settings.theme}
                onChange={(event) =>
                  update("theme", event.target.value as AppSettings["theme"])
                }
              >
                <option value="dark">Escuro</option>
                <option value="system">Sistema</option>
              </select>
            </label>
            <label>
              <div><strong>Iniciar com o sistema</strong><small>Abre o Orbit oculto na bandeja do KDE.</small></div>
              <input type="checkbox" checked={productStatus?.autostart ?? false} onChange={(event)=>void backend.setAutostart(event.target.checked).then(()=>setProductStatus(status=>status ? {...status,autostart:event.target.checked}:status))}/>
            </label>
            <div className="backup-actions">
              <div><strong>Backup e importação</strong><small>Snapshot consistente da biblioteca e configurações.</small></div>
              <button onClick={()=>void (async()=>{const path=await save({title:"Exportar backup do Orbit",defaultPath:"orbit-backup.orbitbackup",filters:[{name:"Backup Orbit",extensions:["orbitbackup"]}]});if(path){await backend.exportBackup(path);setProductMessage("Backup exportado com sucesso.")}})()}>Exportar</button>
              <button onClick={()=>void (async()=>{const path=await open({title:"Importar backup do Orbit",multiple:false,directory:false,filters:[{name:"Backup Orbit",extensions:["orbitbackup"]}]});if(path && window.confirm("A biblioteca atual será substituída. Continuar?")){await backend.importBackup(path);setProductMessage("Backup importado. Reinicie o Orbit.")}})()}>Importar</button>
            </div>
            {productMessage && <p className="report">{productMessage}</p>}
            <div className="backup-actions"><div><strong>Atualizações assinadas</strong><small>{updateStatus?.availableVersion ? `Versão ${updateStatus.availableVersion} disponível` : updateStatus?.configured ? `Orbit ${updateStatus.currentVersion} está atualizado` : "Canal não configurado neste build"}</small></div><button onClick={()=>void backend.checkUpdates().then(setUpdateStatus).catch(error=>setProductMessage(String(error)))}>Verificar</button>{updateStatus?.availableVersion && updateStatus.canInstall && <button onClick={()=>void backend.installUpdate().then(()=>setProductMessage("Atualização instalada. Reinicie o Orbit."))}>Instalar</button>}</div>
            <label>
              <div>
                <strong>Atualizar ao iniciar</strong>
                <small>Escanear providers quando o Orbit abrir.</small>
              </div>
              <input
                type="checkbox"
                checked={settings.scanOnStartup}
                onChange={(event) =>
                  update("scanOnStartup", event.target.checked)
                }
              />
            </label>
            <label>
              <div>
                <strong>Confirmar remoção</strong>
                <small>
                  Pedir confirmação antes de remover itens personalizados.
                </small>
              </div>
              <input
                type="checkbox"
                checked={settings.confirmBeforeRemove}
                onChange={(event) =>
                  update("confirmBeforeRemove", event.target.checked)
                }
              />
            </label>
            <label>
              <div>
                <strong>Terminal preferido</strong>
                <small>
                  Usado por scripts configurados para abrir em terminal.
                </small>
              </div>
              <select
                value={settings.preferredTerminal ?? ""}
                onChange={(event) =>
                  update("preferredTerminal", event.target.value || null)
                }
              >
                <option value="">Automático</option>
                <option value="konsole">Konsole</option>
                <option value="kitty">Kitty</option>
                <option value="alacritty">Alacritty</option>
                <option value="foot">Foot</option>
              </select>
            </label>
          </section>
        ) : section === "themes" ? <ThemesPanel /> : <section className="settings-panel compatibility-panel">
          <div className="panel-heading"><div><h2>CachyOS e Wayland</h2><p>Componentes detectados no host e runtimes disponíveis.</p></div></div>
          {!compatibility ? <p>Verificando componentes…</p> : <>
            <div className="compat-status">{([['GameMode',compatibility.gamemode],['MangoHud',compatibility.mangohud],['Gamescope',compatibility.gamescope],['DXVK',compatibility.dxvk],['VKD3D',compatibility.vkd3d],['Wayland',compatibility.wayland]] as [string,boolean][]).map(([name,ok])=><span className={ok ? 'available':'missing'} key={name}>{ok ? '✓':'—'} {name}</span>)}</div>
            <div className="system-summary"><strong>{compatibility.desktop || 'Desktop não identificado'}</strong><small>Sessão {compatibility.sessionType || 'desconhecida'} · Terminal {compatibility.terminal ?? 'não detectado'}</small></div>
            <h3>Runtimes</h3><div className="runtime-list">{compatibility.runtimes.length ? compatibility.runtimes.map(runtime=><article key={runtime.id}><div><strong>{runtime.name}</strong><small>{runtime.family} · {runtime.managed ? 'gerenciado pelo Orbit':'instalação externa'}</small></div><button onClick={()=>void backend.openPath(runtime.path)}>Abrir pasta</button></article>) : <p>Nenhum Wine/Proton detectado nos diretórios suportados.</p>}</div>
            <button className="icon" onClick={()=>void backend.openPath(compatibility.prefixRoot)}><Library size={16}/>Abrir prefixos gerenciados</button>
          </>}
        </section>}
      </div>
    </div>
  );
}
function PaletteIcon() { return <span aria-hidden="true">◐</span>; }
