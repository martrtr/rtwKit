export interface PortableLayoutOverrides {
  splitWeights: Record<string, number[]>;
  gridWeights: Record<string, number[]>;
  childOrder: Record<string, string[]>;
}

export const EMPTY_PORTABLE_LAYOUT_OVERRIDES: PortableLayoutOverrides = {
  splitWeights: {},
  gridWeights: {},
  childOrder: {},
};

export function portableLayoutStorageKey(
  ownerInstanceId: string,
  surfaceId: string,
): string {
  return `rintawa.web.portable-layout.v1:${ownerInstanceId}:${surfaceId}`;
}

export function readPortableLayoutOverrides(
  storageKey: string,
): PortableLayoutOverrides {
  if (typeof window === "undefined") return EMPTY_PORTABLE_LAYOUT_OVERRIDES;

  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return EMPTY_PORTABLE_LAYOUT_OVERRIDES;
    const parsed = JSON.parse(raw) as Partial<PortableLayoutOverrides>;
    return {
      splitWeights: parsed.splitWeights ?? {},
      gridWeights: parsed.gridWeights ?? {},
      childOrder: parsed.childOrder ?? {},
    };
  } catch {
    return EMPTY_PORTABLE_LAYOUT_OVERRIDES;
  }
}

export function writePortableLayoutOverrides(
  storageKey: string,
  overrides: PortableLayoutOverrides,
): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(overrides));
  } catch {
    // Layout persistence is best-effort; rendering must keep working without storage.
  }
}

export function effectiveWeights(
  defaults: readonly number[],
  override: readonly number[] | undefined,
): number[] {
  if (
    override?.length === defaults.length &&
    override.every((weight) => Number.isFinite(weight) && weight > 0)
  ) {
    return [...override];
  }
  return [...defaults];
}

export function effectiveChildOrder(
  defaults: readonly string[],
  override: readonly string[] | undefined,
): string[] {
  if (!override) return [...defaults];

  const available = new Set(defaults);
  const seen = new Set<string>();
  const ordered = override.filter((childId) => {
    if (!available.has(childId) || seen.has(childId)) return false;
    seen.add(childId);
    return true;
  });

  for (const childId of defaults) {
    if (!seen.has(childId)) ordered.push(childId);
  }
  return ordered;
}

export interface AdjacentResizeBounds {
  minimumFirstPixels: number;
  minimumSecondPixels: number;
  maximumFirstPixels?: number;
  maximumSecondPixels?: number;
}

export function resizeAdjacentWeightsWithinBounds(
  weights: readonly number[],
  index: number,
  deltaPixels: number,
  pairPixels: number,
  bounds: AdjacentResizeBounds,
): number[] {
  if (index < 0 || index + 1 >= weights.length || pairPixels <= 0) {
    return [...weights];
  }

  const minimumFirst = Math.max(0, bounds.minimumFirstPixels);
  const minimumSecond = Math.max(0, bounds.minimumSecondPixels);
  const maximumFirst = Math.max(
    minimumFirst,
    bounds.maximumFirstPixels ?? Number.POSITIVE_INFINITY,
  );
  const maximumSecond = Math.max(
    minimumSecond,
    bounds.maximumSecondPixels ?? Number.POSITIVE_INFINITY,
  );
  const lowerBound = Math.max(minimumFirst, pairPixels - maximumSecond);
  const upperBound = Math.min(maximumFirst, pairPixels - minimumSecond);
  if (lowerBound > upperBound) return [...weights];

  const pairWeight = weights[index]! + weights[index + 1]!;
  const firstPixels = (weights[index]! / pairWeight) * pairPixels;
  const nextFirstPixels = Math.min(
    upperBound,
    Math.max(lowerBound, firstPixels + deltaPixels),
  );
  const nextFirstWeight = (nextFirstPixels / pairPixels) * pairWeight;

  const next = [...weights];
  next[index] = nextFirstWeight;
  next[index + 1] = pairWeight - nextFirstWeight;
  return next;
}

export function resizeAdjacentWeights(
  weights: readonly number[],
  index: number,
  deltaPixels: number,
  pairPixels: number,
  minimumPanePixels: number,
): number[] {
  return resizeAdjacentWeightsWithinBounds(
    weights,
    index,
    deltaPixels,
    pairPixels,
    {
      minimumFirstPixels: minimumPanePixels,
      minimumSecondPixels: minimumPanePixels,
    },
  );
}
