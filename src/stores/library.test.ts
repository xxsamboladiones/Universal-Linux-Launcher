import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LibraryItem } from "../types/library";
import type { TransferOperation } from "../types/platform";

const mocks = vi.hoisted(() => ({
  queueStoreOperation: vi.fn(),
  uninstall: vi.fn(),
  list: vi.fn(),
}));

vi.mock("../services/backend", () => ({
  backend: {
    queueStoreOperation: mocks.queueStoreOperation,
    uninstall: mocks.uninstall,
    list: mocks.list,
  },
}));

import { useLibrary } from "./library";

const epicGame: LibraryItem = {
  id: "epic:game-id",
  name: "Epic Game",
  kind: "game",
  provider: "epic",
  executable: "/managed/legendary",
  arguments: [],
  workingDirectory: null,
  environment: {},
  icon: null,
  cover: null,
  background: null,
  category: "Epic Games",
  tags: [],
  favorite: false,
  hidden: false,
  owned: true,
  installed: false,
  playCount: 0,
  totalPlayTimeSeconds: 0,
  lastPlayedAt: null,
  createdAt: "",
  updatedAt: "",
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
};

const operation = (
  state: TransferOperation["state"],
  error: string | null = null,
): TransferOperation => ({
  id: "operation-id",
  provider: "epic",
  itemId: "game-id",
  action: "install",
  state,
  downloadedBytes: 0,
  totalBytes: 0,
  bytesPerSecond: 0,
  error,
  createdAt: "",
  updatedAt: "",
});

describe("Epic installation state", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useLibrary.setState({ installing: {}, error: null });
  });

  it("keeps the item busy until uninstall and library refresh complete", async () => {
    let finish: (() => void) | undefined;
    mocks.uninstall.mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    mocks.list.mockResolvedValue([{ ...epicGame, installed: false }]);
    useLibrary.setState({ uninstalling: {}, running: {} });

    const request = useLibrary
      .getState()
      .uninstall({ ...epicGame, installed: true });
    expect(useLibrary.getState().uninstalling[epicGame.id]).toBe(true);
    finish?.();
    expect(await request).toBe(true);

    expect(mocks.uninstall).toHaveBeenCalledWith(epicGame.id);
    expect(useLibrary.getState().items[0]?.installed).toBe(false);
    expect(useLibrary.getState().uninstalling[epicGame.id]).toBeUndefined();
  });

  it("keeps the download disabled after it enters the backend queue", async () => {
    mocks.queueStoreOperation.mockResolvedValue("operation-id");

    expect(await useLibrary.getState().install(epicGame)).toBe(true);
    expect(useLibrary.getState().installing[epicGame.id]).toBe(true);
    expect(await useLibrary.getState().install(epicGame)).toBe(false);
    expect(mocks.queueStoreOperation).toHaveBeenCalledOnce();
  });

  it("clears the pending state when the transfer completes", () => {
    useLibrary.getState().applyTransfer(operation("running"));
    expect(useLibrary.getState().installing[epicGame.id]).toBe(true);

    useLibrary.getState().applyTransfer(operation("completed"));
    expect(useLibrary.getState().installing[epicGame.id]).toBeUndefined();
  });

  it("exposes a failed download and makes retrying possible", () => {
    useLibrary.getState().applyTransfer(operation("running"));
    useLibrary
      .getState()
      .applyTransfer(operation("failed", "Falha no download"));

    expect(useLibrary.getState().installing[epicGame.id]).toBeUndefined();
    expect(useLibrary.getState().error).toBe("Falha no download");
  });
});
