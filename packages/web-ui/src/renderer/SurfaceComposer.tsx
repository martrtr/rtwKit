import { useMemo } from "react";
import type { CSSProperties, ReactNode } from "react";

import type { UiActionEvent, UiPresentationSurface } from "../bridge";
import {
  STANDARD_EXPERIENCE_PACK,
  regionForSurface,
} from "../experience";
import type {
  ShellLayoutNode,
  WebExperiencePack,
} from "../experience/types";
import { PortableSurface } from "./PortableSurface";

export type SurfacesByRegion = Map<string, UiPresentationSurface[]>;

export function groupSurfacesByRegion(
  surfaces: readonly UiPresentationSurface[],
  pack: WebExperiencePack,
): SurfacesByRegion {
  const regions: SurfacesByRegion = new Map();

  for (const surface of surfaces) {
    const region = regionForSurface(pack, surface);
    const current = regions.get(region);
    if (current) {
      current.push(surface);
    } else {
      regions.set(region, [surface]);
    }
  }

  return regions;
}

interface RegionProps {
  region: string;
  surfaces: readonly UiPresentationSurface[];
  onAction: (event: UiActionEvent) => void;
  style?: CSSProperties;
}

function Region({ region, surfaces, onAction, style }: RegionProps) {
  if (surfaces.length === 0) return null;

  return (
    <section
      className="rintawa-shell-region"
      data-shell-region={region}
      aria-label={region}
      style={style}
    >
      {surfaces.map((surface) => (
        <PortableSurface
          key={`${surface.owner.instance_id}:${surface.snapshot.surface_id}`}
          surface={surface}
          onAction={onAction}
        />
      ))}
    </section>
  );
}

function renderLayoutNode(
  node: ShellLayoutNode,
  regions: SurfacesByRegion,
  onAction: (event: UiActionEvent) => void,
  key: string,
): ReactNode {
  if (node.type === "region") {
    const surfaces = regions.get(node.region) ?? [];
    if (surfaces.length === 0) return null;

    return (
      <Region
        key={key}
        region={node.region}
        surfaces={surfaces}
        onAction={onAction}
        style={{ flexGrow: node.weight ?? 1 }}
      />
    );
  }

  const children = node.children
    .map((child, index) =>
      renderLayoutNode(child, regions, onAction, `${key}.${index}`),
    )
    .filter((child) => child !== null);

  if (children.length === 0) return null;
  if (children.length === 1) return children[0];

  return (
    <div
      key={key}
      className="rintawa-shell-split"
      data-axis={node.axis}
    >
      {children}
    </div>
  );
}

interface SurfaceComposerProps {
  surfaces: readonly UiPresentationSurface[];
  onAction: (event: UiActionEvent) => void;
  experiencePack?: WebExperiencePack;
}

export function SurfaceComposer({
  surfaces,
  onAction,
  experiencePack = STANDARD_EXPERIENCE_PACK,
}: SurfaceComposerProps) {
  const regions = useMemo(
    () => groupSurfacesByRegion(surfaces, experiencePack),
    [surfaces, experiencePack],
  );
  const workspace = renderLayoutNode(
    experiencePack.shell.workspace,
    regions,
    onAction,
    "workspace",
  );

  const activeLayers = experiencePack.shell.layers.filter(
    (layer) => (regions.get(layer.region)?.length ?? 0) > 0,
  );

  return (
    <div
      className="rintawa-composer"
      data-experience-pack={experiencePack.id}
    >
      {workspace ? (
        <div className="rintawa-shell-workspace">{workspace}</div>
      ) : null}

      {activeLayers.map((layer) => (
        <div
          key={layer.region}
          className="rintawa-shell-layer"
          data-presentation={layer.presentation}
          data-layer-region={layer.region}
        >
          <Region
            region={layer.region}
            surfaces={regions.get(layer.region) ?? []}
            onAction={onAction}
          />
        </div>
      ))}
    </div>
  );
}
