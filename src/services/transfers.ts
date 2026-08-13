import type { TransferOperation } from "../types/platform";

export function transferProgress(operation: TransferOperation): number {
  if (operation.totalBytes <= 0) return 0;
  return Math.min(
    100,
    Math.max(0, (operation.downloadedBytes / operation.totalBytes) * 100),
  );
}

export function remainingSeconds(operation: TransferOperation): number | null {
  if (
    operation.bytesPerSecond <= 0 ||
    operation.downloadedBytes >= operation.totalBytes
  )
    return null;
  return Math.ceil(
    (operation.totalBytes - operation.downloadedBytes) /
      operation.bytesPerSecond,
  );
}
