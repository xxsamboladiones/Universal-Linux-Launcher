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
  Square,
  Trash2,
} from "lucide-react";
import { usePlatform } from "../stores/platform";
import type {
  OperationState,
  StoreAccount,
  StoreId,
  TransferOperation,
} from "../types/platform";
import { backend } from "../services/backend";
import { transferProgress } from "../services/transfers";

const formatBytes = (bytes: number) => `${(bytes / 1_000_000).toFixed(0)} MB`;
const formatTransferBytes = (bytes: number) => {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const unit = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1_000)),
    units.length - 1,
  );
  const value = bytes / 1_000 ** unit;
  const digits = unit === 0 || value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[unit]}`;
};
const operationStateLabel: Record<OperationState, string> = {
  queued: "Na fila",
  running: "Baixando",
  cancelling: "Cancelando",
  cancelled: "Cancelado",
  paused: "Pausado",
  completed: "Concluído",
  failed: "Falhou",
};
const stateLabel: Record<StoreAccount["state"], string> = {
  disconnected: "Não conectado",
  component_required: "Componente necessário",
  connected: "Conectado",
  error: "Atenção necessária",
};
type DependencyProgress = {
  provider: StoreId;
  dependency: string;
  stage: "resolving" | "downloading" | "verifying" | "installing" | "completed";
  downloadedBytes: number;
  totalBytes: number;
};

const progressLabel = (progress: DependencyProgress | null) => {
  if (!progress) return "Preparando…";
  if (progress.stage === "resolving") return "Preparando download…";
  if (progress.stage === "verifying") return "Verificando segurança…";
  if (progress.stage === "installing") return "Instalando…";
  if (progress.stage === "completed") return "Concluindo…";
  if (progress.totalBytes > 0) {
    const percent = Math.min(100, Math.round((progress.downloadedBytes / progress.totalBytes) * 100));
    return `Baixando ${percent}%`;
  }
  return "Baixando…";
};

export function PlatformsPage() {
  const [operationProvider,setOperationProvider]=useState<StoreId>("epic");
  const [operationItem,setOperationItem]=useState("");
  const [dependencyProgress, setDependencyProgress] = useState<
    Partial<Record<StoreId, DependencyProgress>>
  >({});
  const {
    overview,
    loading,
    preparing,
    syncing,
    removing,
    error,
    notice,
    load,
    prepare,
    connect,
    syncLibrary,
    retry,
    cancel,
    remove,
    applyOperationProgress,
  } = usePlatform();
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    const unlisten = listen<TransferOperation>("transfer-progress", (event) => {
      applyOperationProgress(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [applyOperationProgress]);
  useEffect(() => {
    const unlisten = listen<DependencyProgress>("dependency-progress", (event) => {
      setDependencyProgress((current) => ({
        ...current,
        [event.payload.provider]: event.payload,
      }));
    });
    return () => { void unlisten.then((stop) => stop()); };
  }, []);

  const connectAccount = async (account: StoreAccount) => {
    if (account.provider === "gog") {
      try {
        await backend.openProviderLogin("gog");
      } catch (error) {
        window.alert(`Não foi possível abrir o login do GOG: ${String(error)}`);
        return;
      }
      const response = window.prompt(
        "Conclua o login no navegador. Na página final, copie a URL completa da barra de endereços e cole aqui.\n\nEla deve começar com https://embed.gog.com/on_login_success?",
      );
      const authorization = response?.trim();
      if (!authorization) return;
      return connect("gog", authorization);
    }
    if (account.provider !== "steam") {
      return connect(account.provider);
    }

    const promptedUser = window.prompt(
      "Usuário Steam (o Steam Guard será solicitado no terminal)",
    );
    const user = promptedUser?.trim();
    if (!user) return Promise.resolve();

    return connect("steam", user);
  };

  const handleAccountAction = (account: StoreAccount) => {
    if (account.state === "component_required") {
      return prepare(account.provider);
    }
    if (account.state === "connected" && account.provider === "epic") {
      return syncLibrary("epic");
    }
    return connectAccount(account);
  };

  const removeOperation = (operation: TransferOperation) => {
    const message = `Remover ${operation.itemId || operation.provider} da fila?`;
    if (!window.confirm(message)) return;
    void remove(operation.id);
  };

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
      {notice && <div className="report" aria-live="polite">{notice}</div>}
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
              disabled={
                loading ||
                Boolean(preparing[account.provider]) ||
                Boolean(syncing[account.provider]) ||
                (account.state === "connected" && account.provider !== "epic")
              }
              aria-busy={Boolean(syncing[account.provider])}
              onClick={() => void handleAccountAction(account)}
            >
              {account.state === "component_required" ? (
                <>
                  <Download size={16} />
                  {preparing[account.provider]
                    ? progressLabel(dependencyProgress[account.provider] ?? null)
                    : "Preparar suporte"}
                </>
              ) : account.state === "connected" && account.provider === "epic" ? <>
                <RefreshCw className={syncing.epic ? "spin" : ""} size={16}/>
                {syncing.epic ? "Sincronizando…" : "Sincronizar biblioteca"}
              </> : (
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
      <div className="operation-create"><select value={operationProvider} onChange={event=>setOperationProvider(event.target.value as StoreId)}><option value="epic">Epic</option><option value="steam">SteamCMD</option><option value="gog">GOG</option></select><input value={operationItem} onChange={event=>setOperationItem(event.target.value)} placeholder="AppID ou identificador do jogo"/><button disabled={!operationItem.trim()} onClick={()=>void backend.queueStoreOperation(operationProvider,operationItem.trim(),"install").then(()=>{setOperationItem("");return load()})}><Download size={15}/>Adicionar instalação</button></div>
      <section className="dependency-list operation-list">
        {overview?.operations.length ? (
          overview.operations.map((operation) => {
            const progress = transferProgress(operation);
            const hasMeasuredProgress = operation.totalBytes > 0;
            const showProgress =
              hasMeasuredProgress &&
              (operation.state === "running" ||
                operation.state === "cancelling" ||
                operation.state === "cancelled" ||
                operation.state === "paused" ||
                operation.state === "completed");

            return (
              <div className="dependency operation-row" key={operation.id}>
                <Boxes size={20} />
                <div className="operation-content">
                  <div className="operation-heading">
                    <div>
                      <strong>{operation.itemId || operation.provider}</strong>
                      <small>
                        {operation.provider} · {operation.action}
                      </small>
                    </div>
                    <span className={`operation-state ${operation.state}`}>
                      {operationStateLabel[operation.state]}
                    </span>
                  </div>
                  {showProgress && (
                    <div className="operation-progress">
                      <div className="operation-progress-copy">
                        <strong>{progress.toFixed(1)}%</strong>
                        <span>
                          {formatTransferBytes(operation.downloadedBytes)} /{" "}
                          {formatTransferBytes(operation.totalBytes)}
                          {operation.bytesPerSecond > 0 &&
                            ` · ${formatTransferBytes(operation.bytesPerSecond)}/s`}
                        </span>
                      </div>
                      <div
                        className="operation-progress-track"
                        role="progressbar"
                        aria-label={`Download de ${operation.itemId || operation.provider}`}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-valuenow={Math.round(progress)}
                      >
                        <div style={{ width: `${progress}%` }} />
                      </div>
                    </div>
                  )}
                  {(operation.state === "running" ||
                    operation.state === "cancelling") &&
                    !hasMeasuredProgress && (
                    <small className="operation-waiting">
                      {operation.state === "cancelling"
                        ? "Aguardando o download encerrar…"
                        : "Aguardando informações do download…"}
                    </small>
                  )}
                  {operation.error && (
                    <small className="operation-error">{operation.error}</small>
                  )}
                </div>
                <div className="operation-actions">
                  {(operation.state === "failed" ||
                    operation.state === "cancelled") && (
                    <button
                      disabled={Boolean(removing[operation.id])}
                      onClick={() => void retry(operation.id)}
                    >
                      Repetir
                    </button>
                  )}
                  {operation.state === "running" && (
                    <button
                      className="operation-cancel"
                      onClick={() => void cancel(operation.id)}
                    >
                      <Square size={12} />
                      Cancelar
                    </button>
                  )}
                  {operation.state === "cancelling" && (
                    <button className="operation-cancel" disabled>
                      <RefreshCw className="operation-removing-icon" size={14} />
                      Cancelando…
                    </button>
                  )}
                  {operation.state !== "running" &&
                    operation.state !== "cancelling" && (
                    <button
                      className="operation-remove"
                      disabled={Boolean(removing[operation.id])}
                      aria-label={`Remover ${operation.itemId || operation.provider}`}
                      title="Remover da fila"
                      onClick={() => removeOperation(operation)}
                    >
                      {removing[operation.id] ? (
                        <>
                          <RefreshCw className="operation-removing-icon" size={14} />
                          Removendo…
                        </>
                      ) : (
                        <>
                          <Trash2 size={14} />
                          Remover
                        </>
                      )}
                    </button>
                  )}
                </div>
              </div>
            );
          })
        ) : (
          <p className="security-note">Nenhuma operação na fila.</p>
        )}
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
        O Orbit baixa componentes de receitas confiáveis, valida o SHA-256 e faz
        a instalação de forma atômica. Tokens de sessão são armazenados via{" "}
        {overview?.credentialStore ?? "Secret Service/KWallet"}, nunca no SQLite.
      </p>
    </div>
  );
}
