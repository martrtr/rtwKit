export interface PortableLayoutOverrides {
  splitWeights: Record<string, number[]>;
  gridWeights: Record<string, number[]>;
}

export const EMPTY_PORTABLE_LAYOUT_OVERRIDES: PortableLayoutOverrides = {
  splitWeights: {},
  gridWeights: {},
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

export function resizeAdjacentWeights(
  weights: readonly number[],
  index: number,
  deltaPixels: number,
  pairPixels: number,
  minimumPanePixels: number,
): number[] {
  if (
    index < 0 ||
    index + 1 >= weights.length ||
    pairPixels <= minimumPanePixels * 2
  ) {
    return [...weights];
  }

  const pairWeight = weights[index]! + weights[index + 1]!;
  const leftPixels = (weights[index]! / pairWeight) * pairPixels;
  const nextLeftPixels = Math.min(
    pairPixels - minimumPanePixels,
    Math.max(minimumPanePixels, leftPixels + deltaPixels),
  );
  const nextLeftWeight = (nextLeftPixels / pairPixels) * pairWeight;

  const next = [...weights];
  next[index] = nextLeftWeight;
  next[index + 1] = pairWeight - nextLeftWeight;
  return next;
}
