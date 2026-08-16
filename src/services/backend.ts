import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  AppSettings,
  ArgumentPreset,
  ItemInput,
  LibraryItem,
  ScanReport,
  CompatibilityOverview,
} from "../types/library";
import type { PlatformOverview, StoreId } from "../types/platform";
export interface ProductStatus {
  autostart: boolean;
  appimage: boolean;
  executable: string;
}
export interface UpdateStatus {
  configured: boolean;
  currentVersion: string;
  availableVersion: string | null;
  canInstall: boolean;
}
const inTauri = () => isTauri();
const demo: LibraryItem[] = [
  {
    id: "custom:welcome",
    name: "Bem-vindo ao Orbit",
    kind: "application",
    provider: "custom",
    executable: null,
    arguments: [],
    workingDirectory: null,
    environment: {},
    icon: null,
    cover: null,
    background: null,
    category: "Introdução",
    tags: [],
    favorite: true,
    hidden: false,
    owned: true,
    installed: true,
    playCount: 0,
    totalPlayTimeSeconds: 0,
    lastPlayedAt: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    terminal: false,
    compatibility: {
      runtimeId: null,
      prefixPath: null,
      steamOverlay: false,
      gamemode: false,
      mangohud: false,
      gamescope: {
        enabled: false,
        width: null,
        height: null,
        outputWidth: null,
        outputHeight: null,
        fps: null,
        fullscreen: false,
        upscaler: null,
      },
      dxvk: false,
      vkd3d: false,
    },
  },
];
const overview: PlatformOverview = {
  accounts: [
    {
      provider: "steam",
      displayName: "Steam",
      description:
        "Downloads por SteamCMD; o cliente Steam pode ser necessário para DRM.",
      state: "component_required",
      librarySize: 0,
      dependencyIds: ["steamcmd"],
      strategy: "native",
    },
    {
      provider: "epic",
      displayName: "Epic Games",
      description: "Biblioteca, instalação e execução por Legendary.",
      state: "component_required",
      librarySize: 0,
      dependencyIds: ["legendary"],
      strategy: "replacement",
    },
    {
      provider: "gog",
      displayName: "GOG",
      description: "Integração preparada para um adaptador autenticado.",
      state: "disconnected",
      librarySize: 0,
      dependencyIds: [],
      strategy: "replacement",
    },
    {
      provider: "battlenet",
      displayName: "Battle.net",
      description: "Cliente oficial isolado em um prefixo Wine gerenciado.",
      state: "component_required",
      librarySize: 0,
      dependencyIds: ["wine-ge", "battlenet-client"],
      strategy: "managed_client",
    },
  ],
  dependencies: [
    {
      id: "steamcmd",
      name: "SteamCMD",
      provider: "steam",
      state: "missing",
      installedVersion: null,
      requiredDiskBytes: 192_000_000,
      executable: null,
    },
    {
      id: "legendary",
      name: "Legendary",
      provider: "epic",
      state: "missing",
      installedVersion: null,
      requiredDiskBytes: 55_000_000,
      executable: null,
    },
    {
      id: "wine-ge",
      name: "Wine-GE",
      provider: "compatibility",
      state: "missing",
      installedVersion: null,
      requiredDiskBytes: 680_000_000,
      executable: null,
    },
    {
      id: "battlenet-client",
      name: "Battle.net Client",
      provider: "battlenet",
      state: "missing",
      installedVersion: null,
      requiredDiskBytes: 500_000_000,
      executable: null,
    },
  ],
  runtimes: [
    {
      id: "system-wine",
      family: "wine",
      name: "Wine do sistema",
      version: "detectado no host",
      installed: false,
      source: "system",
    },
    {
      id: "proton-ge",
      family: "proton",
      name: "GE-Proton",
      version: "não instalado",
      installed: false,
      source: "managed",
    },
  ],
  operations: [],
  credentialStore: "Secret Service",
};
export const backend = {
  list: () =>
    inTauri() ? invoke<LibraryItem[]>("get_library") : Promise.resolve(demo),
  scan: () =>
    inTauri()
      ? invoke<ScanReport>("scan_providers")
      : Promise.reject(
          new Error("O scan exige o aplicativo Tauri. Execute pnpm tauri dev."),
        ),
  launch: (id: string) => invoke<number>("launch_item", { id }),
  running: () => invoke<Record<string, number>>("get_running_items"),
  save: (item: ItemInput) =>
    inTauri()
      ? invoke<LibraryItem>("update_item", { item })
      : Promise.reject(new Error("O cadastro exige o aplicativo Tauri.")),
  favorite: (id: string, value: boolean) =>
    invoke<void>("set_favorite", { id, value }),
  hide: (id: string, value: boolean) =>
    invoke<void>("set_hidden", { id, value }),
  delete: (id: string) => invoke<void>("delete_item", { id }),
  uninstall: (id: string) => invoke<void>("uninstall_item", { id }),
  settings: () => invoke<AppSettings>("get_settings"),
  updateSettings: (settings: AppSettings) =>
    invoke<void>("update_settings", { settings }),
  platformOverview: () =>
    inTauri()
      ? invoke<PlatformOverview>("get_platform_overview")
      : Promise.resolve(overview),
  prepareProvider: (provider: StoreId) =>
    invoke<void>("prepare_provider", { provider }),
  connectProvider: (provider: StoreId, user?: string) =>
    invoke<void>("connect_provider", { provider, user }),
  queueStoreOperation: (
    provider: StoreId,
    itemId: string,
    action: "install" | "update" | "verify",
  ) => invoke<string>("queue_store_operation", { provider, itemId, action }),
  retryOperation: (id: string) => invoke<void>("retry_operation", { id }),
  removeStoreOperation: (id: string) =>
    invoke<void>("remove_store_operation", { id }),
  cancelStoreOperation: (id: string) =>
    invoke<void>("cancel_store_operation", { id }),
  syncStoreLibrary: (provider: StoreId) =>
    invoke<number>("sync_store_library", { provider }),
  rollbackDependency: (id: string) =>
    invoke<void>("rollback_dependency", { id }),
  compatibility: () =>
    invoke<CompatibilityOverview>("get_compatibility_overview"),
  createPrefix: (id: string) => invoke<string>("create_game_prefix", { id }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  openCompatibilityLog: (id: string) =>
    invoke<void>("open_compatibility_log", { id }),
  productStatus: () => invoke<ProductStatus>("get_product_status"),
  setAutostart: (enabled: boolean) =>
    invoke<void>("set_autostart", { enabled }),
  exportBackup: (path: string) => invoke<void>("export_backup", { path }),
  importBackup: (path: string) => invoke<void>("import_backup", { path }),
  checkUpdates: () => invoke<UpdateStatus>("check_for_updates"),
  installUpdate: () => invoke<void>("install_update"),
  listArgumentPresets: () =>
    invoke<ArgumentPreset[]>("list_argument_presets"),
  saveArgumentPreset: (preset: ArgumentPreset) =>
    invoke<void>("save_argument_preset", { preset }),
  deleteArgumentPreset: (id: string) =>
    invoke<void>("delete_argument_preset", { id }),
  getArgumentPreset: (id: string) =>
    invoke<ArgumentPreset | null>("get_argument_preset", { id }),
};
