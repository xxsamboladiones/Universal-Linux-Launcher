import { describe, expect, it } from "vitest";
import { remainingSeconds, transferProgress } from "./transfers";
import type { TransferOperation } from "../types/platform";

const operation: TransferOperation = {
  id: "1",
  provider: "epic",
  itemId: "Example",
  action: "install",
  state: "running",
  downloadedBytes: 75,
  totalBytes: 100,
  bytesPerSecond: 5,
  error: null,
  createdAt: "",
  updatedAt: "",
};
describe("transfer calculations", () => {
  it("calculates bounded progress", () => {
    expect(transferProgress(operation)).toBe(75);
    expect(transferProgress({ ...operation, downloadedBytes: 120 })).toBe(100);
  });
  it("calculates remaining time", () => {
    expect(remainingSeconds(operation)).toBe(5);
    expect(remainingSeconds({ ...operation, bytesPerSecond: 0 })).toBeNull();
  });
});
