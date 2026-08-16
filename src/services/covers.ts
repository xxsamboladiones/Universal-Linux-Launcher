import type { LibraryItem } from "../types/library";

const legacyGogBackground =
  /^https:\/\/images-[1-4]\.gog-statics\.com\//i;

export function usableCover(item: LibraryItem): string | null {
  if (
    item.provider === "gog" &&
    item.cover &&
    legacyGogBackground.test(item.cover)
  ) {
    return null;
  }
  return item.cover;
}
