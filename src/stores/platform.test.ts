import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PlatformOverview } from "../types/platform";
import type { TransferOperation } from "../types/platform";

const mocks = vi.hoisted(() => ({
  connectProvider: vi.fn(),
  platformOverview: vi.fn(),
  removeStoreOperation: vi.fn(),
  cancelStoreOperation: vi.fn(),
  syncStoreLibrary: vi.fn(),
  list: vi.fn(),
}));

vi.mock("../services/backend", () => ({
  backend: {
    connectProvider: mocks.connectProvider,
    platformOverview: mocks.platformOverview,
    removeStoreOperation: mocks.removeStoreOperation,
    cancelStoreOperation: mocks.cancelStoreOperation,
    syncStoreLibrary: mocks.syncStoreLibrary,
    list: mocks.list,
  },
}));

import { usePlatform } from "./platform";

const connectedOverview: PlatformOverview = {
  accounts: [
    {
      provider: "steam",
      displayName: "Steam",
      description: "SteamCMD",
      state: "connected",
      librarySize: 0,
      dependencyIds: ["steamcmd"],
      strategy: "native",
    },
  ],
  dependencies: [],
  runtimes: [],
  operations: [],
  credentialStore: "KWallet",
};

describe("platform authentication", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    usePlatform.setState({
      overview: null,
      loading: false,
      preparing: {},
      syncing: {},
      removing: {},
      error: null,
      notice: null,
    });
  });

  it("waits for Steam authentication and reloads the connection state", async () => {
    mocks.connectProvider.mockResolvedValue(undefined);
    mocks.platformOverview.mockResolvedValue(connectedOverview);

    await usePlatform.getState().connect("steam", "orbit_user");

    expect(mocks.connectProvider).toHaveBeenCalledWith("steam", "orbit_user");
    expect(mocks.platformOverview).toHaveBeenCalledOnce();
    expect(usePlatform.getState().overview?.accounts[0]?.state).toBe("connected");
    expect(usePlatform.getState().loading).toBe(false);
  });

  it("applies transfer progress without reloading the overview", () => {
    const initialOperation: TransferOperation = {
      id: "download-1",
      provider: "steam",
      itemId: "645630",
      action: "install",
      state: "running",
      downloadedBytes: 0,
      totalBytes: 0,
      bytesPerSecond: 0,
      error: null,
      createdAt: "2026-08-13T15:00:00Z",
      updatedAt: "2026-08-13T15:00:00Z",
    };
    const progress = {
      ...initialOperation,
      downloadedBytes: 500,
      totalBytes: 1_000,
      bytesPerSecond: 100,
    };
    usePlatform.setState({
      overview: {
        ...connectedOverview,
        operations: [initialOperation],
      },
    });

    usePlatform.getState().applyOperationProgress(progress);

    expect(usePlatform.getState().overview?.operations).toEqual([progress]);
    expect(mocks.platformOverview).not.toHaveBeenCalled();
  });

  it("keeps Epic sync responsive and rejects duplicate clicks", async () => {
    let completeSync: ((count: number) => void) | undefined;
    mocks.syncStoreLibrary.mockReturnValue(
      new Promise<number>((resolve) => {
        completeSync = resolve;
      }),
    );
    mocks.platformOverview.mockResolvedValue(connectedOverview);
    mocks.list.mockResolvedValue([]);

    const first = usePlatform.getState().syncLibrary("epic");
    const duplicate = usePlatform.getState().syncLibrary("epic");

    expect(usePlatform.getState().syncing.epic).toBe(true);
    expect(mocks.syncStoreLibrary).toHaveBeenCalledTimes(1);
    completeSync?.(190);
    await Promise.all([first, duplicate]);

    expect(usePlatform.getState().syncing.epic).toBeUndefined();
    expect(usePlatform.getState().notice).toContain("190 jogos sincronizados");
    expect(mocks.platformOverview).toHaveBeenCalledOnce();
    expect(mocks.list).toHaveBeenCalledOnce();
  });

  it("clears Epic sync state and displays backend failures", async () => {
    mocks.syncStoreLibrary.mockRejectedValue(
      new Error("Legendary indisponível"),
    );

    await usePlatform.getState().syncLibrary("epic");

    expect(usePlatform.getState().syncing.epic).toBeUndefined();
    expect(usePlatform.getState().error).toContain("Legendary indisponível");
    expect(usePlatform.getState().notice).toBeNull();
  });

  it("does not announce success when the refreshed library cannot be loaded", async () => {
    mocks.syncStoreLibrary.mockResolvedValue(190);
    mocks.platformOverview.mockResolvedValue(connectedOverview);
    mocks.list.mockRejectedValue(new Error("falha ao atualizar a biblioteca"));

    await usePlatform.getState().syncLibrary("epic");

    expect(usePlatform.getState().notice).toBeNull();
    expect(usePlatform.getState().error).toContain(
      "falha ao atualizar a biblioteca",
    );
    expect(usePlatform.getState().syncing.epic).toBeUndefined();
  });

  it("removes an operation from the queue after backend confirmation", async () => {
    const operation: TransferOperation = {
      id: "download-2",
      provider: "steam",
      itemId: "1050280",
      action: "install",
      state: "failed",
      downloadedBytes: 0,
      totalBytes: 0,
      bytesPerSecond: 0,
      error: "exit status: 8",
      createdAt: "2026-08-13T15:00:00Z",
      updatedAt: "2026-08-13T15:00:00Z",
    };
    mocks.removeStoreOperation.mockResolvedValue(undefined);
    usePlatform.setState({
      overview: { ...connectedOverview, operations: [operation] },
    });

    const request = usePlatform.getState().remove(operation.id);
    expect(usePlatform.getState().removing[operation.id]).toBe(true);
    await request;

    expect(mocks.removeStoreOperation).toHaveBeenCalledWith(operation.id);
    expect(usePlatform.getState().overview?.operations).toEqual([]);
    expect(usePlatform.getState().removing[operation.id]).toBeUndefined();
  });

  it("keeps an operation visible when removal fails", async () => {
    const operation: TransferOperation = {
      id: "download-3",
      provider: "steam",
      itemId: "645630",
      action: "install",
      state: "queued",
      downloadedBytes: 0,
      totalBytes: 0,
      bytesPerSecond: 0,
      error: null,
      createdAt: "2026-08-13T15:00:00Z",
      updatedAt: "2026-08-13T15:00:00Z",
    };
    mocks.removeStoreOperation.mockRejectedValue(new Error("operação em uso"));
    usePlatform.setState({
      overview: { ...connectedOverview, operations: [operation] },
    });

    await usePlatform.getState().remove(operation.id);

    expect(usePlatform.getState().overview?.operations).toEqual([operation]);
    expect(usePlatform.getState().error).toContain("operação em uso");
    expect(usePlatform.getState().removing[operation.id]).toBeUndefined();
  });

  it("marks a running operation as cancelling and reloads it as cancelled", async () => {
    const operation: TransferOperation = {
      id: "download-4",
      provider: "steam",
      itemId: "1050280",
      action: "install",
      state: "running",
      downloadedBytes: 500,
      totalBytes: 1_000,
      bytesPerSecond: 100,
      error: null,
      createdAt: "2026-08-13T15:00:00Z",
      updatedAt: "2026-08-13T15:00:00Z",
    };
    const cancelled = { ...operation, state: "cancelled" as const };
    mocks.cancelStoreOperation.mockResolvedValue(undefined);
    mocks.platformOverview.mockResolvedValue({
      ...connectedOverview,
      operations: [cancelled],
    });
    usePlatform.setState({
      overview: { ...connectedOverview, operations: [operation] },
    });

    const request = usePlatform.getState().cancel(operation.id);
    expect(usePlatform.getState().overview?.operations[0]?.state).toBe(
      "cancelling",
    );
    await request;

    expect(mocks.cancelStoreOperation).toHaveBeenCalledWith(operation.id);
    expect(usePlatform.getState().overview?.operations).toEqual([cancelled]);
  });

  it("restores a running operation when cancellation fails", async () => {
    const operation: TransferOperation = {
      id: "download-5",
      provider: "steam",
      itemId: "645630",
      action: "install",
      state: "running",
      downloadedBytes: 0,
      totalBytes: 0,
      bytesPerSecond: 0,
      error: null,
      createdAt: "2026-08-13T15:00:00Z",
      updatedAt: "2026-08-13T15:00:00Z",
    };
    mocks.cancelStoreOperation.mockRejectedValue(new Error("não cancelou"));
    mocks.platformOverview.mockResolvedValue({
      ...connectedOverview,
      operations: [operation],
    });
    usePlatform.setState({
      overview: { ...connectedOverview, operations: [operation] },
    });

    await usePlatform.getState().cancel(operation.id);

    expect(usePlatform.getState().overview?.operations).toEqual([operation]);
    expect(usePlatform.getState().error).toContain("não cancelou");
  });
});
