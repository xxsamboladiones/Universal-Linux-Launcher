import type { LibraryItem } from "../types/library";

/**
 * Aggregated views contain only locally installed entries. Provider catalog
 * views may expose owned titles that still need to be downloaded.
 */
export function visibleInLibrary(item: LibraryItem, filter: string): boolean {
  if (item.hidden) return false;

  if (filter === "epic") {
    return item.provider === "epic" && item.owned;
  }

  if (!item.installed) return false;

  return (
    filter === "all" ||
    filter === "home" ||
    (filter === "favorites" && item.favorite) ||
    item.kind === filter ||
    item.provider === filter
  );
}
