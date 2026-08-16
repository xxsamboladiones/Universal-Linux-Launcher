import { Download, Palette, Trash2 } from "lucide-react";
import type { ThemeSummary } from "../types";

interface Props {
  theme: ThemeSummary;
  selected: boolean;
  busy: boolean;
  onApply(): void;
  onExport(): void;
  onRemove(): void;
}

export function ThemeCard({
  theme,
  selected,
  busy,
  onApply,
  onExport,
  onRemove,
}: Props) {
  return (
    <article className={`theme-card ${selected ? "active" : ""}`}>
      <div
        className={`theme-preview ${theme.type}`}
        style={{
          background: theme.previewUrl
            ? undefined
            : "linear-gradient(145deg, var(--orbit-color-primary), var(--orbit-color-surface))",
        }}
      >
        {theme.previewUrl ? (
          <img
            src={theme.previewUrl}
            alt={`Prévia do tema ${theme.name}`}
            loading="lazy"
          />
        ) : (
          <Palette size={30} aria-hidden="true" />
        )}
      </div>
      <div className="theme-card-copy">
        <div>
          <strong>{theme.name}</strong>
          <small>
            por {theme.author} · v{theme.version}
          </small>
        </div>
        <span className={`theme-type ${theme.type}`}>
          {theme.type === "dark" ? "Escuro" : "Claro"}
        </span>
      </div>
      <p>{theme.description}</p>
      <div className="theme-actions">
        <button
          className={selected ? "selected" : ""}
          disabled={busy || selected || !theme.compatible}
          onClick={onApply}
        >
          {selected ? "✓ Ativo" : "Aplicar"}
        </button>
        {theme.source === "external" && (
          <>
            <button title="Exportar tema" disabled={busy} onClick={onExport}>
              <Download size={15} />
            </button>
            <button
              className="danger"
              title="Remover tema"
              disabled={busy}
              onClick={onRemove}
            >
              <Trash2 size={15} />
            </button>
          </>
        )}
      </div>
      <small className="theme-origin">
        {theme.source === "builtin" ? "Interno" : "Externo"} · Orbit{" "}
        {theme.orbitVersion}
      </small>
    </article>
  );
}
