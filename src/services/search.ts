import type { LibraryItem } from "../types/library";
export function matches(item: LibraryItem, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return true;
  return [item.name, item.provider, item.category ?? "", ...item.tags].some(
    (value) => value.toLocaleLowerCase().includes(needle),
  );
}
