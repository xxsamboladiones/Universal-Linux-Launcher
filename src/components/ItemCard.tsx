import { EyeOff, Heart, Pencil, Play, Trash2 } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { LibraryItem } from "../types/library";

interface Props {
  item: LibraryItem;
  running: boolean;
  onLaunch: () => void;
  onFavorite: () => void;
  onHide: () => void;
  onEdit: () => void;
  onDelete: () => void;
}
export function ItemCard({
  item,
  running,
  onLaunch,
  onFavorite,
  onHide,
  onEdit,
  onDelete,
}: Props) {
  const icon = item.icon?.startsWith("/") ? convertFileSrc(item.icon) : null;
  const cover = item.cover?.startsWith("/")
    ? convertFileSrc(item.cover)
    : item.cover;
  return (
    <article className={`card ${running ? "is-running" : ""}`}>
      <div
        className="art"
        style={cover ? { backgroundImage: `url(${cover})` } : undefined}
      >
        {icon ? (
          <img className="resolved-icon" src={icon} alt="" />
        ) : (
          <div className="monogram">{item.name.slice(0, 2).toUpperCase()}</div>
        )}
        <button
          aria-label={running ? "Em execução" : "Jogar"}
          className="play"
          disabled={!item.executable || running}
          onClick={onLaunch}
        >
          {running ? (
            <span className="running-dot" />
          ) : (
            <Play fill="currentColor" />
          )}
        </button>
      </div>
      <div className="card-info">
        <div>
          <strong>{item.name}</strong>
          <small>
            {running
              ? "Em execução"
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
        {item.provider === "custom" && (
          <button aria-label="Remover" onClick={onDelete}>
            <Trash2 size={18} />
          </button>
        )}
      </div>
    </article>
  );
}
