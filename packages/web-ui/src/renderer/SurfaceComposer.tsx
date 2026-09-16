import { useMemo } from "react";

import type { UiActionEvent, UiPresentationSurface } from "../bridge";
import type { UiPlacementHint } from "../bridge/types";
import { PortableSurface } from "./PortableSurface";

export type SurfaceGroups = Record<UiPlacementHint, UiPresentationSurface[]>;

export function groupSurfacesByPlacement(
  surfaces: readonly UiPresentationSurface[],
): SurfaceGroups {
  const groups: SurfaceGroups = {
    primary: [],
    secondary: [],
    sidebar: [],
    settings: [],
    dialog: [],
    status: [],
    overlay: [],
  };

  for (const surface of surfaces) {
    groups[surface.contribution.placement].push(surface);
  }

  return groups;
}

interface SurfaceGroupProps {
  placement: UiPlacementHint;
  label: string;
  surfaces: readonly UiPresentationSurface[];
  onAction: (event: UiActionEvent) => void;
}

function SurfaceGroup({ placement, label, surfaces, onAction }: SurfaceGroupProps) {
  if (surfaces.length === 0) return null;

  return (
    <section
      className="rintawa-surface-group"
      data-placement-group={placement}
      aria-label={label}
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

interface SurfaceComposerProps {
  surfaces: readonly UiPresentationSurface[];
  onAction: (event: UiActionEvent) => void;
}

export function SurfaceComposer({ surfaces, onAction }: SurfaceComposerProps) {
  const groups = useMemo(() => groupSurfacesByPlacement(surfaces), [surfaces]);
  const hasMainSurfaces =
    groups.primary.length > 0 ||
    groups.secondary.length > 0 ||
    groups.settings.length > 0;
  const hasSidebarSurfaces = groups.sidebar.length > 0;
  const hasWorkspaceSurfaces = hasMainSurfaces || hasSidebarSurfaces;
  const hasFloatingSurfaces = groups.dialog.length > 0 || groups.overlay.length > 0;

  return (
    <div className="rintawa-composer">
      {hasWorkspaceSurfaces ? (
        <div
          className="rintawa-workspace"
          data-layout={hasMainSurfaces && hasSidebarSurfaces ? "split" : "single"}
        >
          {hasMainSurfaces ? (
            <div className="rintawa-main-stack">
              <SurfaceGroup
                placement="primary"
                label="Primary content"
                surfaces={groups.primary}
                onAction={onAction}
              />
              <SurfaceGroup
                placement="secondary"
                label="Secondary content"
                surfaces={groups.secondary}
                onAction={onAction}
              />
              <SurfaceGroup
                placement="settings"
                label="Settings"
                surfaces={groups.settings}
                onAction={onAction}
              />
            </div>
          ) : null}
          <SurfaceGroup
            placement="sidebar"
            label="Sidebar"
            surfaces={groups.sidebar}
            onAction={onAction}
          />
        </div>
      ) : null}

      <SurfaceGroup
        placement="status"
        label="Status"
        surfaces={groups.status}
        onAction={onAction}
      />

      {hasFloatingSurfaces ? (
        <div className="rintawa-floating-layer">
          <SurfaceGroup
            placement="dialog"
            label="Dialogs"
            surfaces={groups.dialog}
            onAction={onAction}
          />
          <SurfaceGroup
            placement="overlay"
            label="Overlays"
            surfaces={groups.overlay}
            onAction={onAction}
          />
        </div>
      ) : null}
    </div>
  );
}
