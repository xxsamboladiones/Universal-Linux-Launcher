import { describe, expect, it } from "vitest";
import { matches } from "./search";
import type { LibraryItem } from "../types/library";
const item = {
  id: "steam:730",
  name: "Counter-Strike 2",
  provider: "steam",
  kind: "game",
  category: "FPS",
  tags: ["competitivo"],
  arguments: [],
  environment: {},
  executable: "steam",
  workingDirectory: null,
  icon: null,
  cover: null,
  background: null,
  favorite: false,
  hidden: false,
  installed: true,
  playCount: 0,
  totalPlayTimeSeconds: 0,
  lastPlayedAt: null,
  createdAt: "",
  updatedAt: "",
  terminal: false,
  compatibility: { runtimeId: null, prefixPath: null, steamOverlay: false, gamemode: false, mangohud: false, gamescope: { enabled: false, width: null, height: null, outputWidth: null, outputHeight: null, fps: null, fullscreen: false, upscaler: null }, dxvk: false, vkd3d: false },
} satisfies LibraryItem;
describe("library search", () => {
  it("matches name, provider, category and tags", () => {
    expect(matches(item, "strike")).toBe(true);
    expect(matches(item, "STEAM")).toBe(true);
    expect(matches(item, "fps")).toBe(true);
    expect(matches(item, "competitivo")).toBe(true);
    expect(matches(item, "rpg")).toBe(false);
  });
});
