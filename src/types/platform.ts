export type StoreId = "steam" | "epic" | "gog" | "battlenet";
export type ConnectionState =
  "disconnected" | "component_required" | "connected" | "error";
export type DependencyState = "missing" | "installed" | "update_available";
export type OperationState =
  | "queued"
  | "running"
  | "cancelling"
  | "cancelled"
  | "paused"
  | "completed"
  | "failed";

export interface StoreAccount {
  provider: StoreId;
  displayName: string;
  description: string;
  state: ConnectionState;
  librarySize: number;
  dependencyIds: string[];
  strategy: "native" | "replacement" | "managed_client";
}

export interface ManagedDependency {
  id: string;
  name: string;
  provider: StoreId | "compatibility";
  state: DependencyState;
  installedVersion: string | null;
  requiredDiskBytes: number;
  executable: string | null;
}

export interface RuntimeVersion {
  id: string;
  family: "proton" | "wine";
  name: string;
  version: string;
  installed: boolean;
  source: string;
}

export interface TransferOperation {
  id: string;
  provider: StoreId;
  itemId: string;
  action: "install" | "update" | "verify";
  state: OperationState;
  downloadedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  error: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface PlatformOverview {
  accounts: StoreAccount[];
  dependencies: ManagedDependency[];
  runtimes: RuntimeVersion[];
  operations: TransferOperation[];
  credentialStore: string;
}
