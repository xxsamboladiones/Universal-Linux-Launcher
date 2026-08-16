import { describe, expect, it } from "vitest";
import type { LibraryItem } from "../types/library";
import { visibleInLibrary } from "./library-filter";

const epicGame = (values: Partial<LibraryItem> = {}): LibraryItem => ({
  id: "epic:example",
  name: "Epic Example",
  kind: "game",
  provider: "epic",
  executable: "/managed/legendary",
  arguments: [],
  workingDirectory: null,
  environment: {},
  icon: null,
  cover: "https://cdn1.epicgames.com/example/portrait.png",
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
  ...values,
});

describe("library provider filters", () => {
  it("shows owned, uninstalled Epic games only in the Epic catalog", () => {
    const game = epicGame();

    expect(visibleInLibrary(game, "epic")).toBe(true);
    expect(visibleInLibrary(game, "game")).toBe(false);
    expect(visibleInLibrary(game, "all")).toBe(false);
  });

  it("keeps installed Epic games in both aggregated and Epic views", () => {
    const game = epicGame({ installed: true });

    expect(visibleInLibrary(game, "epic")).toBe(true);
    expect(visibleInLibrary(game, "game")).toBe(true);
    expect(visibleInLibrary(game, "all")).toBe(true);
  });

  it("does not expose games that no longer belong to the account", () => {
    expect(visibleInLibrary(epicGame({ owned: false }), "epic")).toBe(false);
    expect(visibleInLibrary(epicGame({ hidden: true }), "epic")).toBe(false);
  });

  it("keeps a physically installed title in aggregate views only", () => {
    const localOnly = epicGame({ owned: false, installed: true });

    expect(visibleInLibrary(localOnly, "game")).toBe(true);
    expect(visibleInLibrary(localOnly, "epic")).toBe(false);
  });

  it("exposes owned uninstalled GOG games in the GOG catalog", () => {
    const game = epicGame({
      id: "gog:1207658997",
      provider: "gog",
      name: "GOG Example",
      category: "GOG",
    });

    expect(visibleInLibrary(game, "gog")).toBe(true);
    expect(visibleInLibrary(game, "all")).toBe(false);
    expect(visibleInLibrary(game, "epic")).toBe(false);
  });
});
