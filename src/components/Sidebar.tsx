import {
  Gamepad2,
  Grid2X2,
  Heart,
  Home,
  LibraryBig,
  Monitor,
  Settings,
  Store,
} from "lucide-react";
import orbitIcon from "../../src-tauri/icons/icon.png";
const nav = [
  ["home", "Início", Home],
  ["all", "Todos", Grid2X2],
  ["game", "Jogos", Gamepad2],
  ["application", "Aplicativos", Monitor],
  ["favorites", "Favoritos", Heart],
  ["steam", "Steam", LibraryBig],
  ["platforms", "Lojas e contas", Store],
] as const;
export function Sidebar({
  active,
  onChange,
}: {
  active: string;
  onChange: (v: string) => void;
}) {
  return (
    <aside>
      <div className="brand">
        <img className="brand-icon" src={orbitIcon} alt="Ícone do Orbit" />
        <div>
          ORBIT<small>UNIVERSAL LAUNCHER</small>
        </div>
      </div>
      <nav>
        <label>BIBLIOTECA</label>
        {nav.map(([id, text, Icon]) => (
          <button
            className={active === id ? "active" : ""}
            key={id}
            onClick={() => onChange(id)}
          >
            <Icon size={18} />
            {text}
          </button>
        ))}
      </nav>
      <button
        className={active === "settings" ? "settings active" : "settings"}
        onClick={() => onChange("settings")}
      >
        <Settings size={18} />
        Configurações
      </button>
    </aside>
  );
}
