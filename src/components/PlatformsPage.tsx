import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Boxes,
  Check,
  Download,
  ExternalLink,
  HardDrive,
  ShieldCheck,
  RefreshCw,
} from "lucide-react";
import { usePlatform } from "../stores/platform";
import type { StoreAccount } from "../types/platform";
import type { StoreId } from "../types/platform";
import { backend } from "../services/backend";

const formatBytes = (bytes: number) => `${(bytes / 1_000_000).toFixed(0)} MB`;
const stateLabel: Record<StoreAccount["state"], string> = {
  disconnected: "Não conectado",
  component_required: "Componente necessário",
  connected: "Conectado",
  error: "Atenção necessária",
};

export function PlatformsPage() {
  const [operationProvider,setOperationProvider]=useState<StoreId>("epic");
  const [operationItem,setOperationItem]=useState("");
  const { overview, loading, error, load, prepare, connect, retry } = usePlatform();
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => { const unlisten=listen("transfer-progress",()=>void load()); return ()=>{void unlisten.then(fn=>fn())}; },[load]);
  return (
    <div className="platform-page">
      <div className="hero compact">
        <p>PLATAFORMAS GERENCIADAS</p>
        <h1>Contas e componentes</h1>
        <span>
          O Orbit instala e isola as ferramentas necessárias sem armazenar suas
          senhas.
        </span>
      </div>
      {error && <div className="report error">{error}</div>}
      <section className="account-grid">
        {overview?.accounts.map((account) => (
          <article className="account" key={account.provider}>
            <div className={`provider-mark ${account.provider}`}>
              {account.displayName.slice(0, 1)}
            </div>
            <div className="account-copy">
              <h3>{account.displayName}</h3>
              <span className={`status ${account.state}`}>
                <i />
                {stateLabel[account.state]}
              </span>
              <p>{account.description}</p>
              <small>
                {account.strategy === "replacement"
                  ? "Cliente oficial dispensável quando suportado"
                  : account.strategy === "managed_client"
                    ? "Cliente oficial encapsulado"
                    : "Compatibilidade híbrida"}
              </small>
            </div>
            <button
              disabled={loading || (account.state === "connected" && account.provider !== "epic")}
              onClick={() => void (account.state === "component_required" ? prepare(account.provider) : account.state === "connected" && account.provider === "epic" ? backend.syncStoreLibrary("epic").then(()=>load()) : account.provider === "steam" ? backend.connectProvider("steam",window.prompt("Usuário Steam (o Steam Guard será solicitado no terminal)") || undefined) : connect(account.provider))}
            >
              {account.state === "component_required" ? (
                <>
                  <Download size={16} />
                  Preparar suporte
                </>
              ) : account.state === "connected" && account.provider === "epic" ? <> <RefreshCw size={16}/>Sincronizar biblioteca</> : (
                <>
                  <ExternalLink size={16} />
                  Conectar
                </>
              )}
            </button>
          </article>
        ))}
      </section>
      <div className="section-title"><div><p>FILA TRANSACIONAL</p><h2>Instalações e atualizações</h2></div><Download size={22}/></div>
      <div className="operation-create"><select value={operationProvider} onChange={event=>setOperationProvider(event.target.value as StoreId)}><option value="epic">Epic</option><option value="steam">SteamCMD</option><option value="gog">GOG</option><option value="battlenet">Battle.net</option></select><input value={operationItem} onChange={event=>setOperationItem(event.target.value)} placeholder="AppID ou identificador do jogo"/><button disabled={!operationItem.trim()} onClick={()=>void backend.queueStoreOperation(operationProvider,operationItem.trim(),"install").then(()=>{setOperationItem("");return load()})}><Download size={15}/>Adicionar instalação</button></div>
      <section className="dependency-list operation-list">
        {overview?.operations.length ? overview.operations.map(operation => <div className="dependency" key={operation.id}><Boxes size={20}/><div><strong>{operation.itemId || operation.provider}</strong><small>{operation.provider} · {operation.action}</small>{operation.error && <small className="operation-error">{operation.error}</small>}</div><span className={operation.state}>{operation.state}</span>{operation.state === "failed" && <button onClick={()=>void retry(operation.id)}>Repetir</button>}</div>) : <p className="security-note">Nenhuma operação na fila.</p>}
      </section>
      <div className="section-title">
        <div>
          <p>DEPENDÊNCIAS</p>
          <h2>Componentes sob controle do Orbit</h2>
        </div>
        <ShieldCheck size={22} />
      </div>
      <section className="dependency-list">
        {overview?.dependencies.map((dep) => (
          <div className="dependency" key={dep.id}>
            <Boxes size={20} />
            <div>
              <strong>{dep.name}</strong>
              <small>
                {dep.provider} · {formatBytes(dep.requiredDiskBytes)}
              </small>
            </div>
            <span className={dep.state}>
              {dep.state === "installed" ? (
                <>
                  <Check size={14} />
                  Instalado
                </>
              ) : (
                "Não instalado"
              )}
            </span>
          </div>
        ))}
      </section>
      <div className="section-title">
        <div>
          <p>COMPATIBILIDADE</p>
          <h2>Wine e Proton gerenciados</h2>
        </div>
        <HardDrive size={22} />
      </div>
      <section className="runtime-grid">
        {overview?.runtimes.map((runtime) => (
          <div className="runtime" key={runtime.id}>
            <strong>{runtime.name}</strong>
            <span>{runtime.version}</span>
            <small>
              {runtime.family.toUpperCase()} · {runtime.source}
            </small>
          </div>
        ))}
      </section>
      <p className="security-note">
        Downloads reais permanecem bloqueados até que a origem, o checksum e a
        assinatura do componente sejam definidos no manifesto. Tokens de sessão
        são armazenados via {overview?.credentialStore ?? "Secret Service/KWallet"}, nunca no SQLite.
      </p>
    </div>
  );
}
