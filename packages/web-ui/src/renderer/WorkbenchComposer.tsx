import { useEffect, useMemo, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";

import type { UiActionEvent, UiPresentationSurface } from "../bridge";
import { regionForSurface } from "../experience";
import type {
  ShellLayoutNode,
  ShellRegionPresentation,
  WebExperiencePack,
} from "../experience/types";
import { InterfaceIcon } from "../icons/InterfaceIcon";
import { PortableSurface } from "./PortableSurface";
import {
  effectiveWeights,
  resizeAdjacentWeightsWithinBounds,
} from "./portableLayout";
import {
  readWorkspacePreferences,
  shellLayoutIdentity,
  shellLayoutLabel,
  shellRegionPresentation,
  shellResizeBounds,
  writeWorkspacePreferences,
} from "./workspaceLayout";
import type { WorkspacePreferences } from "./workspaceLayout";

export interface DerivedActivity {
  key: string;
  id: string;
  label: string;
  iconSlot: string | null;
  surfaces: UiPresentationSurface[];
  defaultRegion: string;
}

function humanizeSurface(surface: UiPresentationSurface): string {
  const source =
    surface.contribution.semantic?.id ??
    surface.contribution.id ??
    surface.snapshot.surface_id;
  const tail = source.split(".").at(-1) ?? source;
  return tail
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function activityKey(surface: UiPresentationSurface, activityId: string): string {
  return `${surface.owner.instance_id}:${activityId}`;
}

function activityDestinations(pack: WebExperiencePack): ShellRegionPresentation[] {
  return (pack.shell.region_presentations ?? []).filter(
    (presentation) => presentation.accepts_activities === true,
  );
}

export function deriveActivities(
  surfaces: readonly UiPresentationSurface[],
  pack: WebExperiencePack,
): DerivedActivity[] {
  const layerRegions = new Set(pack.shell.layers.map((layer) => layer.region));
  const destinations = new Set(activityDestinations(pack).map((item) => item.region));
  const activities = new Map<string, DerivedActivity>();

  for (const surface of surfaces) {
    const routedRegion = regionForSurface(pack, surface);
    if (layerRegions.has(routedRegion)) continue;

    const declared = surface.contribution.activity;
    const activityId = declared?.id ?? surface.contribution.id;
    const key = activityKey(surface, activityId);
    const defaultRegion = destinations.has(routedRegion)
      ? routedRegion
      : pack.shell.fallback_region;
    const existing = activities.get(key);

    if (existing) {
      existing.surfaces.push(surface);
      continue;
    }

    activities.set(key, {
      key,
      id: activityId,
      label: declared?.label ?? humanizeSurface(surface),
      iconSlot: declared?.icon_slot ?? "activity.extension",
      surfaces: [surface],
      defaultRegion,
    });
  }

  return [...activities.values()];
}

function layerSurfaces(
  surfaces: readonly UiPresentationSurface[],
  pack: WebExperiencePack,
  region: string,
): UiPresentationSurface[] {
  return surfaces.filter((surface) => regionForSurface(pack, surface) === region);
}

interface ActivityRegionProps {
  region: string;
  presentation: ShellRegionPresentation | undefined;
  activities: readonly DerivedActivity[];
  activeKey: string | undefined;
  collapsed: boolean;
  onFocus: (activity: DerivedActivity) => void;
  onToggleCollapsed: () => void;
  onOpenMenu: (activity: DerivedActivity, anchor: HTMLElement) => void;
  onAction: (event: UiActionEvent) => void;
}

function ActivityRegion({
  region,
  presentation,
  activities,
  activeKey,
  collapsed,
  onFocus,
  onToggleCollapsed,
  onOpenMenu,
  onAction,
}: ActivityRegionProps) {
  if (presentation?.collapsible && (collapsed || activities.length === 0)) {
    return null;
  }

  const active =
    activities.find((activity) => activity.key === activeKey) ?? activities[0];

  return (
    <section
      className="rintawa-workbench-region"
      data-shell-region={region}
      data-region-mode={presentation?.mode ?? "plain"}
    >
      {presentation?.mode === "activity-tabs" ? (
        <div className="rintawa-workbench-tabs" role="tablist" aria-label={presentation.label}>
          <div className="rintawa-workbench-tab-strip">
            {activities.map((activity) => (
              <button
                key={activity.key}
                type="button"
                role="tab"
                aria-selected={active?.key === activity.key}
                className="rintawa-workbench-tab"
                data-active={active?.key === activity.key}
                onClick={() => onFocus(activity)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  onOpenMenu(activity, event.currentTarget);
                }}
              >
                <InterfaceIcon slot={activity.iconSlot} size={16} />
                <span>{activity.label}</span>
              </button>
            ))}
          </div>
          {presentation.collapsible ? (
            <button
              type="button"
              className="rintawa-workbench-chrome-button"
              title={`Collapse ${presentation.label}`}
              aria-label={`Collapse ${presentation.label}`}
              onClick={onToggleCollapsed}
            >
              <span aria-hidden>×</span>
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="rintawa-workbench-region-content">
        {active ? (
          active.surfaces.map((surface) => (
            <PortableSurface
              key={`${surface.owner.instance_id}:${surface.snapshot.surface_id}`}
              surface={surface}
              onAction={onAction}
            />
          ))
        ) : (
          <div className="rintawa-workbench-empty-region">
            {presentation?.label ?? region}
          </div>
        )}
      </div>
    </section>
  );
}

function renderWorkbenchLayout(
  node: ShellLayoutNode,
  pack: WebExperiencePack,
  assigned: ReadonlyMap<string, DerivedActivity[]>,
  activeByRegion: Readonly<Record<string, string>>,
  collapsed: Readonly<Record<string, boolean>>,
  splitWeights: Readonly<Record<string, number[]>>,
  onFocus: (activity: DerivedActivity) => void,
  onToggleCollapsed: (region: string) => void,
  onResizeSplit: (splitId: string, weights: number[]) => void,
  onOpenMenu: (activity: DerivedActivity, anchor: HTMLElement) => void,
  onAction: (event: UiActionEvent) => void,
  key: string,
): ReactNode {
  if (node.type === "region") {
    const presentation = shellRegionPresentation(pack, node);
    return (
      <ActivityRegion
        key={key}
        region={node.region}
        presentation={presentation}
        activities={assigned.get(node.region) ?? []}
        activeKey={activeByRegion[node.region]}
        collapsed={collapsed[node.region] ?? false}
        onFocus={onFocus}
        onToggleCollapsed={() => onToggleCollapsed(node.region)}
        onOpenMenu={onOpenMenu}
        onAction={onAction}
      />
    );
  }

  const splitId = shellLayoutIdentity(node);
  const defaultWeights = node.children.map((child) =>
    child.type === "region" ? child.weight ?? 1 : 1,
  );
  const weights = effectiveWeights(defaultWeights, splitWeights[splitId]);
  const visibleChildren = node.children
    .map((child, sourceIndex) => ({
      child,
      sourceIndex,
      content: renderWorkbenchLayout(
        child,
        pack,
        assigned,
        activeByRegion,
        collapsed,
        splitWeights,
        onFocus,
        onToggleCollapsed,
        onResizeSplit,
        onOpenMenu,
        onAction,
        `${key}.${sourceIndex}`,
      ),
    }))
    .filter((entry) => entry.content !== null);

  if (visibleChildren.length === 0) return null;
  if (visibleChildren.length === 1) return visibleChildren[0]!.content;

  const resizeVisiblePair = (
    visibleIndex: number,
    deltaPixels: number,
    pairPixels: number,
  ) => {
    const first = visibleChildren[visibleIndex];
    const second = visibleChildren[visibleIndex + 1];
    if (!first || !second) return;

    const firstBounds = shellResizeBounds(pack, first.child);
    const secondBounds = shellResizeBounds(pack, second.child);
    const pair = [weights[first.sourceIndex]!, weights[second.sourceIndex]!];
    const resized = resizeAdjacentWeightsWithinBounds(
      pair,
      0,
      deltaPixels,
      pairPixels,
      {
        minimumFirstPixels: firstBounds.minimum,
        minimumSecondPixels: secondBounds.minimum,
        maximumFirstPixels: firstBounds.maximum,
        maximumSecondPixels: secondBounds.maximum,
      },
    );
    const next = [...weights];
    next[first.sourceIndex] = resized[0]!;
    next[second.sourceIndex] = resized[1]!;
    onResizeSplit(splitId, next);
  };

  const adjacentPaneGeometry = (
    handle: HTMLButtonElement,
    visibleIndex: number,
  ) => {
    const split = handle.parentElement;
    if (!split) return null;
    const panes = [...split.children].filter((element) =>
      element.classList.contains("rintawa-shell-split-pane"),
    );
    const first = panes[visibleIndex]?.getBoundingClientRect();
    const second = panes[visibleIndex + 1]?.getBoundingClientRect();
    if (!first || !second) return null;
    const pairPixels =
      node.axis === "horizontal"
        ? first.width + second.width
        : first.height + second.height;
    return { pairPixels };
  };

  const resizeByKeyboard = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    visibleIndex: number,
    deltaPixels: number,
  ) => {
    const geometry = adjacentPaneGeometry(event.currentTarget, visibleIndex);
    if (!geometry) return;
    resizeVisiblePair(visibleIndex, deltaPixels, geometry.pairPixels);
  };

  const beginResize = (
    event: ReactPointerEvent<HTMLButtonElement>,
    visibleIndex: number,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    const geometry = adjacentPaneGeometry(event.currentTarget, visibleIndex);
    if (!geometry) return;

    const horizontal = node.axis === "horizontal";
    const startCoordinate = horizontal ? event.clientX : event.clientY;
    const pairPixels = geometry.pairPixels;

    const move = (pointerEvent: PointerEvent) => {
      const coordinate = horizontal ? pointerEvent.clientX : pointerEvent.clientY;
      resizeVisiblePair(
        visibleIndex,
        coordinate - startCoordinate,
        pairPixels,
      );
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
  };

  return (
    <div key={key} className="rintawa-shell-split" data-axis={node.axis}>
      {visibleChildren.flatMap((entry, visibleIndex) => {
        const pane = (
          <div
            key={shellLayoutIdentity(entry.child)}
            className="rintawa-shell-split-pane"
            style={{ flexGrow: weights[entry.sourceIndex] ?? 1 }}
          >
            {entry.content}
          </div>
        );
        if (visibleIndex === visibleChildren.length - 1) return [pane];

        const next = visibleChildren[visibleIndex + 1]!;
        return [
          pane,
          <button
            key={`${shellLayoutIdentity(entry.child)}:resize`}
            type="button"
            className="rintawa-shell-split-resize-handle"
            data-axis={node.axis}
            aria-label={`Resize ${shellLayoutLabel(pack, entry.child)} and ${shellLayoutLabel(pack, next.child)}`}
            onPointerDown={(event) => beginResize(event, visibleIndex)}
            onKeyDown={(event) => {
              const decrease = node.axis === "horizontal" ? "ArrowLeft" : "ArrowUp";
              const increase = node.axis === "horizontal" ? "ArrowRight" : "ArrowDown";
              if (event.key === decrease) {
                event.preventDefault();
                resizeByKeyboard(event, visibleIndex, -40);
              } else if (event.key === increase) {
                event.preventDefault();
                resizeByKeyboard(event, visibleIndex, 40);
              }
            }}
          />,
        ];
      })}
    </div>
  );
}

interface WorkbenchComposerProps {
  surfaces: readonly UiPresentationSurface[];
  onAction: (event: UiActionEvent) => void;
  experiencePack: WebExperiencePack;
}

export function WorkbenchComposer({
  surfaces,
  onAction,
  experiencePack,
}: WorkbenchComposerProps) {
  const activities = useMemo(
    () => deriveActivities(surfaces, experiencePack),
    [surfaces, experiencePack],
  );
  const destinations = useMemo(
    () => activityDestinations(experiencePack),
    [experiencePack],
  );
  const destinationIds = useMemo(
    () => new Set(destinations.map((destination) => destination.region)),
    [destinations],
  );
  const [preferences, setPreferences] = useState<WorkspacePreferences>(() =>
    readWorkspacePreferences(experiencePack),
  );
  const [menu, setMenu] = useState<{
    activityKey: string;
    left: number;
    top: number;
  } | null>(null);

  useEffect(() => {
    setPreferences(readWorkspacePreferences(experiencePack));
    setMenu(null);
  }, [experiencePack]);

  useEffect(() => {
    writeWorkspacePreferences(experiencePack, preferences);
  }, [experiencePack, preferences]);

  const assigned = useMemo(() => {
    const regions = new Map<string, DerivedActivity[]>();
    for (const activity of activities) {
      const requested = preferences.placements[activity.key];
      const region =
        requested && destinationIds.has(requested)
          ? requested
          : activity.defaultRegion;
      const target = destinationIds.has(region)
        ? region
        : experiencePack.shell.fallback_region;
      const bucket = regions.get(target);
      if (bucket) bucket.push(activity);
      else regions.set(target, [activity]);
    }
    return regions;
  }, [
    activities,
    destinationIds,
    experiencePack.shell.fallback_region,
    preferences.placements,
  ]);

  const effectiveActiveByRegion = useMemo(() => {
    const active = { ...preferences.activeByRegion };
    for (const [region, regionActivities] of assigned) {
      if (!regionActivities.some((activity) => activity.key === active[region])) {
        const first = regionActivities[0];
        if (first) active[region] = first.key;
      }
    }
    return active;
  }, [assigned, preferences.activeByRegion]);

  const updatePreferences = (
    update: (current: WorkspacePreferences) => WorkspacePreferences,
  ) => setPreferences((current) => update(current));

  const focusActivity = (activity: DerivedActivity) => {
    const override = preferences.placements[activity.key];
    const region =
      override && destinationIds.has(override) ? override : activity.defaultRegion;
    updatePreferences((current) => ({
      ...current,
      activeByRegion: { ...current.activeByRegion, [region]: activity.key },
      collapsed: { ...current.collapsed, [region]: false },
    }));
  };

  const moveActivity = (activity: DerivedActivity, region: string) => {
    if (!destinationIds.has(region)) return;
    updatePreferences((current) => ({
      ...current,
      placements: { ...current.placements, [activity.key]: region },
      activeByRegion: { ...current.activeByRegion, [region]: activity.key },
      collapsed: { ...current.collapsed, [region]: false },
    }));
    setMenu(null);
  };

  const toggleCollapsed = (region: string) => {
    updatePreferences((current) => ({
      ...current,
      collapsed: {
        ...current.collapsed,
        [region]: !(current.collapsed[region] ?? false),
      },
    }));
  };

  const resizeSplit = (splitId: string, weights: number[]) => {
    if (!weights.every((weight) => Number.isFinite(weight) && weight > 0)) return;
    updatePreferences((current) => ({
      ...current,
      splitWeights: {
        ...current.splitWeights,
        [splitId]: weights,
      },
    }));
  };

  const openMenu = (activity: DerivedActivity, anchor: HTMLElement) => {
    const rect = anchor.getBoundingClientRect();
    setMenu({
      activityKey: activity.key,
      left: Math.min(rect.right + 6, window.innerWidth - 220),
      top: Math.min(rect.top, window.innerHeight - 180),
    });
  };

  const menuActivity = menu
    ? activities.find((activity) => activity.key === menu.activityKey)
    : undefined;

  const workspace = renderWorkbenchLayout(
    experiencePack.shell.workspace,
    experiencePack,
    assigned,
    effectiveActiveByRegion,
    preferences.collapsed,
    preferences.splitWeights,
    focusActivity,
    toggleCollapsed,
    resizeSplit,
    openMenu,
    onAction,
    "workspace",
  );

  const activeLayers = experiencePack.shell.layers.filter(
    (layer) => layerSurfaces(surfaces, experiencePack, layer.region).length > 0,
  );

  return (
    <div className="rintawa-workbench" data-experience-pack={experiencePack.id}>
      {experiencePack.shell.activity_bar?.presentation !== "hidden" ? (
        <nav
          className="rintawa-activity-rail"
          data-presentation={
            experiencePack.shell.activity_bar?.presentation ?? "vertical-start"
          }
          aria-label="Activities"
        >
          {activities.map((activity) => {
            const override = preferences.placements[activity.key];
            const region =
              override && destinationIds.has(override)
                ? override
                : activity.defaultRegion;
            const isActive =
              effectiveActiveByRegion[region] === activity.key &&
              !(preferences.collapsed[region] ?? false);
            return (
              <button
                key={activity.key}
                type="button"
                className="rintawa-activity-button"
                data-active={isActive}
                title={activity.label}
                aria-label={activity.label}
                onClick={() => focusActivity(activity)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  openMenu(activity, event.currentTarget);
                }}
              >
                <InterfaceIcon slot={activity.iconSlot} size={22} />
              </button>
            );
          })}
        </nav>
      ) : null}

      <div className="rintawa-workbench-body">
        <div className="rintawa-shell-workspace">{workspace}</div>

        {activeLayers.map((layer) => (
          <div
            key={layer.region}
            className="rintawa-shell-layer"
            data-presentation={layer.presentation}
            data-layer-region={layer.region}
          >
            <section
              className="rintawa-shell-region"
              data-shell-region={layer.region}
              aria-label={layer.region}
            >
              {layerSurfaces(surfaces, experiencePack, layer.region).map(
                (surface) => (
                  <PortableSurface
                    key={`${surface.owner.instance_id}:${surface.snapshot.surface_id}`}
                    surface={surface}
                    onAction={onAction}
                  />
                ),
              )}
            </section>
          </div>
        ))}
      </div>

      {menu && menuActivity ? (
        <>
          <button
            type="button"
            className="rintawa-workbench-menu-backdrop"
            aria-label="Close activity menu"
            onClick={() => setMenu(null)}
          />
          <div
            className="rintawa-workbench-menu"
            role="menu"
            style={{ left: menu.left, top: menu.top }}
          >
            <div className="rintawa-workbench-menu-title">
              {menuActivity.label}
            </div>
            {destinations.map((destination) => (
              <button
                key={destination.region}
                type="button"
                role="menuitem"
                onClick={() => moveActivity(menuActivity, destination.region)}
              >
                Move to {destination.label}
              </button>
            ))}
          </div>
        </>
      ) : null}
    </div>
  );
}
