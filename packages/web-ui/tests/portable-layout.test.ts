import { describe, expect, it } from "vitest";

import {
  effectiveWeights,
  resizeAdjacentWeights,
} from "../src/renderer/portableLayout";

describe("portable layout overrides", () => {
  it("uses matching positive overrides and rejects stale shapes", () => {
    expect(effectiveWeights([1, 2, 3], [3, 2, 1])).toEqual([3, 2, 1]);
    expect(effectiveWeights([1, 2, 3], [4, 5])).toEqual([1, 2, 3]);
    expect(effectiveWeights([1, 2], [1, 0])).toEqual([1, 2]);
  });

  it("resizes only the adjacent pair while preserving its total weight", () => {
    const result = resizeAdjacentWeights([1, 3, 5], 0, 100, 400, 40);

    expect(result[0]).toBeCloseTo(2);
    expect(result[1]).toBeCloseTo(2);
    expect(result[2]).toBe(5);
    expect(result[0]! + result[1]!).toBeCloseTo(4);
  });

  it("clamps adjacent panes to a usable minimum", () => {
    const result = resizeAdjacentWeights([1, 1], 0, -10_000, 200, 60);

    expect(result[0]).toBeCloseTo(0.6);
    expect(result[1]).toBeCloseTo(1.4);
  });
});
