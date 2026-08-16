import { useState } from "react";
import {
  Download,
  EyeOff,
  Heart,
  PackageX,
  Pencil,
  Play,
  Trash2,
} from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { LibraryItem } from "../types/library";

interface Props {
  item: LibraryItem;
  running: boolean;
  onLaunch: () => void;
  onInstall: () => void;
  installing: boolean;
  uninstalling: boolean;
  onFavorite: () => void;
  onHide: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onUninstall: () => void;
}
export function ItemCard({
  item,
  running,
  onLaunch,
  onInstall,
  installing,
  uninstalling,
  onFavorite,
  onHide,
  onEdit,
  onDelete,
  onUninstall,
}: Props) {
  const icon = item.icon?.startsWith("/") ? convertFileSrc(item.icon) : null;
  const cover = item.cover?.startsWith("/")
    ? convertFileSrc(item.cover)
    : item.cover;
  const [failedCover, setFailedCover] = useState<string | null>(null);
  const visibleCover = cover && cover !== failedCover ? cover : null;
  const managedDownload = ["epic", "gog"].includes(item.provider);
  const canActivate = item.installed
    ? Boolean(item.executable) && !running && !uninstalling
    : managedDownload && !installing;
  const activate = () => {
    if (!canActivate) return;
    if (item.installed) onLaunch();
    else onInstall();
  };
  return (
    <article className={`card ${running ? "is-running" : ""}`}>
      <div
        className={`art ${canActivate ? "actionable" : ""}`}
        role={canActivate ? "button" : undefined}
        tabIndex={canActivate ? 0 : undefined}
        aria-label={
          canActivate
            ? item.installed
              ? `Jogar ${item.name}`
              : `Instalar ${item.name}`
            : undefined
        }
        onClick={activate}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            activate();
          }
        }}
      >
        {visibleCover && (
          <img
            className="cover-image"
            src={visibleCover}
            alt=""
            onError={() => setFailedCover(visibleCover)}
          />
        )}
        {icon ? (
          <img className="resolved-icon" src={icon} alt="" />
        ) : !visibleCover ? (
          <div className="monogram">{item.name.slice(0, 2).toUpperCase()}</div>
        ) : null}
        {item.installed ? (
          <button
            aria-label={running ? "Em execução" : "Jogar"}
            className="play"
            disabled={!item.executable || running}
            onClick={(event) => {
              event.stopPropagation();
              onLaunch();
            }}
          >
            {running ? (
              <span className="running-dot" />
            ) : (
              <Play fill="currentColor" />
            )}
          </button>
        ) : managedDownload ? (
          <button
            aria-label={installing ? "Instalação na fila" : "Baixar"}
            className="play install"
            disabled={installing}
            onClick={(event) => {
              event.stopPropagation();
              onInstall();
            }}
          >
            <Download />
          </button>
        ) : null}
      </div>
      <div className="card-info">
        <div>
          <strong>{item.name}</strong>
          <small>
            {running
              ? "Em execução"
              : uninstalling
                ? "Desinstalando…"
                : installing
                  ? `${item.provider === "epic" ? "Epic" : "GOG"} · Instalação na fila`
                  : !item.installed
                    ? `${item.provider} · Não instalado`
                    : `${item.provider} · ${item.category ?? item.kind}`}
          </small>
        </div>
        <button
          aria-label="Favoritar"
          className={item.favorite ? "liked" : ""}
          onClick={onFavorite}
        >
          <Heart size={18} fill={item.favorite ? "currentColor" : "none"} />
        </button>
        <button aria-label="Ocultar" onClick={onHide}>
          <EyeOff size={18} />
        </button>
        <button aria-label="Editar" onClick={onEdit}>
          <Pencil size={18} />
        </button>
        {item.installed &&
          ["epic", "gog", "steam", "flatpak", "appimage"].includes(
            item.provider,
          ) && (
            <button
              aria-label="Desinstalar"
              title="Desinstalar"
              disabled={running || uninstalling}
              onClick={onUninstall}
            >
              <PackageX size={18} />
            </button>
          )}
        {item.provider === "custom" && (
          <button aria-label="Remover" onClick={onDelete}>
            <Trash2 size={18} />
          </button>
        )}
      </div>
    </article>
  );
}
