import { describe, expect, it } from "vitest";
import type { LibraryItem } from "../types/library";
import { usableCover } from "./covers";

const item = (provider: string, cover: string | null) =>
  ({ provider, cover }) as LibraryItem;

describe("usableCover", () => {
  it("hides legacy GOG page backgrounds", () => {
    expect(
      usableCover(
        item(
          "gog",
          "https://images-1.gog-statics.com/deprecated-background.jpg",
        ),
      ),
    ).toBeNull();
  });

  it("keeps vertical GamesDB covers and other provider artwork", () => {
    expect(
      usableCover(
        item(
          "gog",
          "https://images.gog.com/vertical.jpg?namespace=gamesdb",
        ),
      ),
    ).toBe("https://images.gog.com/vertical.jpg?namespace=gamesdb");
    expect(
      usableCover(
        item("epic", "https://images-1.gog-statics.com/unrelated.jpg"),
      ),
    ).toBe("https://images-1.gog-statics.com/unrelated.jpg");
  });
});
