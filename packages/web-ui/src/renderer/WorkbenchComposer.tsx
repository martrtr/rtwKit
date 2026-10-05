import { useEffect, useMemo, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";

import type {
  UiActionEvent,
  UiLayerPresentationState,
  UiManagementContext,
  UiPresentationSurface,
} from "../bridge";
import { regionForSurface } from "../experience";
import type {
  ShellLayoutNode,
  ShellRegionPresentation,
  WebExperiencePack,
} from "../experience/types";
import { InterfaceIcon } from "../icons/InterfaceIcon";
import { PortableSurface } from "./PortableSurface";
import { beginWindowPointerResize } from "./pointerResize";
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

function activityKey(
  surface: UiPresentationSurface,
  activityId: string,
): string {
  if (surface.contribution.activity) {
    return `activity:${activityId}`;
  }
  return `surface:${surface.owner.instance_id}:${activityId}`;
}

function activitySurfaceKey(surface: UiPresentationSurface): string {
  return `${surface.owner.instance_id}:${surface.contribution.id}`;
}

const MANAGEMENT_ACTIVITY_ID = "rintawa.management";
const MANAGEMENT_CONTEXT_SEMANTIC_ID = "rintawa.management.context";

function hostManagementContext(revision: number): UiManagementContext {
  return {
    revision,
    scope_id: "host",
    label: "Manage",
    world_id: null,
  };
}

export function managementContextAction(
  surface: UiPresentationSurface,
  scopeId: string,
): UiActionEvent | null {
  const sink = surface.snapshot.nodes.find(
    (node) =>
      node.semantic?.id === MANAGEMENT_CONTEXT_SEMANTIC_ID &&
      node.semantic.version === 1 &&
      node.kind.type === "text-input",
  );
  if (!sink || sink.kind.type !== "text-input" || !sink.kind.data.is_enabled) return null;
  if (sink.kind.data.value === scopeId || sink.kind.data.change_action === null) return null;
  return {
    owner_instance_id: surface.owner.instance_id,
    surface_id: surface.snapshot.surface_id,
    node_id: sink.id,
    action_id: sink.kind.data.change_action,
    surface_revision: surface.snapshot.revision,
    payload: { type: "text", value: scopeId },
  };
}

function activityDestinations(
  pack: WebExperiencePack,
): ShellRegionPresentation[] {
  return (pack.shell.region_presentations ?? []).filter(
    (presentation) => presentation.accepts_activities === true,
  );
}

export function deriveActivities(
  surfaces: readonly UiPresentationSurface[],
  pack: WebExperiencePack,
): DerivedActivity[] {
  const layerRegions = new Set(pack.shell.layers.map((layer) => layer.region));
  const destinations = new Set(
    activityDestinations(pack).map((item) => item.region),
  );
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
    const label = declared?.label ?? humanizeSurface(surface);
    const iconSlot = declared?.icon_slot ?? "activity.extension";
    const existing = activities.get(key);

    const isActivitySection = surface.contribution.traits.includes("activity-section");
    const existingIsActivitySection = existing?.surfaces.every((candidate) =>
      candidate.contribution.traits.includes("activity-section"),
    );
    if (
      existing &&
      existing.label === label &&
      existing.iconSlot === iconSlot &&
      existing.defaultRegion === defaultRegion &&
      existingIsActivitySection === isActivitySection
    ) {
      existing.surfaces.push(surface);
      continue;
    }

    const effectiveKey = existing
      ? `${key}:${surface.owner.instance_id}:${surface.contribution.id}`
      : key;
    activities.set(effectiveKey, {
      key: effectiveKey,
      id: activityId,
      label,
      iconSlot,
      surfaces: [surface],
      defaultRegion,
    });
  }

  return [...activities.values()];
}

export function deriveGlobalActivities(
  surfaces: readonly UiPresentationSurface[],
  pack: WebExperiencePack,
): DerivedActivity[] {
  return deriveActivities(
    surfaces.filter(
      (surface) =>
        surface.context?.kind !== "focused-world" &&
        surface.contribution.activity !== null,
    ),
    pack,
  );
}

export function deriveFocusedWorldActivities(
  surfaces: readonly UiPresentationSurface[],
  pack: WebExperiencePack,
): DerivedActivity[] {
  return deriveActivities(
    surfaces.filter(
      (surface) =>
        surface.context?.kind === "focused-world" &&
        surface.contribution.activity !== null,
    ),
    pack,
  );
}

export function focusedWorldEntryActivity(
  surfaces: readonly UiPresentationSurface[],
  activities: readonly DerivedActivity[],
  presentation: UiLayerPresentationState,
  pack: WebExperiencePack,
): DerivedActivity | null {
  const focused = presentation.focused_world;
  if (!focused) return null;
  const entry = surfaces.find(
    (surface) =>
      surface.context?.kind === "focused-world" &&
      surface.context.world_id === focused.world_id &&
      surface.contribution.id === focused.descriptor.entry_surface_id,
  );
  if (!entry) return null;
  const entryWorkspaceActivity = activities.find((activity) =>
    activity.surfaces.some(
      (surface) =>
        surface.owner.instance_id === entry.owner.instance_id &&
        surface.contribution.id === entry.contribution.id,
    ),
  );
  if (entryWorkspaceActivity) return entryWorkspaceActivity;

  const routedRegion = regionForSurface(pack, entry);
  if (pack.shell.layers.some((layer) => layer.region === routedRegion))
    return null;
  const destinations = new Set(
    activityDestinations(pack).map((item) => item.region),
  );
  return {
    key: `world-entry:${focused.world_id}:${entry.owner.instance_id}:${entry.contribution.id}`,
    id: `world-entry:${focused.world_id}`,
    label: humanizeSurface(entry),
    iconSlot: null,
    surfaces: [entry],
    defaultRegion: destinations.has(routedRegion)
      ? routedRegion
      : pack.shell.fallback_region,
  };
}

function layerSurfaces(
  surfaces: readonly UiPresentationSurface[],
  pack: WebExperiencePack,
  region: string,
): UiPresentationSurface[] {
  return surfaces.filter(
    (surface) => regionForSurface(pack, surface) === region,
  );
}

interface ActivityRegionProps {
  region: string;
  presentation: ShellRegionPresentation | undefined;
  activities: readonly DerivedActivity[];
  tabActivities: readonly DerivedActivity[];
  activeKey: string | undefined;
  collapsed: boolean;
  sectionNavigationWidth: number | undefined;
  onFocus: (activity: DerivedActivity) => void;
  onToggleCollapsed: () => void;
  onResizeSectionNavigation: (activityKey: string, width: number) => void;
  onOpenMenu: (activity: DerivedActivity, anchor: HTMLElement) => void;
  onAction: (event: UiActionEvent) => void;
  managementContext: UiManagementContext;
  onExitWorldManagement: () => void;
}

function ActivityRegion({
  region,
  presentation,
  activities,
  tabActivities,
  activeKey,
  collapsed,
  sectionNavigationWidth,
  onFocus,
  onToggleCollapsed,
  onResizeSectionNavigation,
  onOpenMenu,
  onAction,
  managementContext,
  onExitWorldManagement,
}: ActivityRegionProps) {
  const [selectedSurfaceByActivity, setSelectedSurfaceByActivity] = useState<
    Record<string, string>
  >({});
  const sectionNavigationRef = useRef<HTMLElement | null>(null);

  const boundedSectionNavigationWidth = (requested: number): number => {
    const navigation = sectionNavigationRef.current;
    if (!navigation || typeof window === "undefined") return Math.max(96, requested);
    const styles = window.getComputedStyle(navigation);
    const minimum = Number.parseFloat(styles.minWidth);
    const maximum = Number.parseFloat(styles.maxWidth);
    const effectiveMinimum = Number.isFinite(minimum) ? minimum : 96;
    const effectiveMaximum = Number.isFinite(maximum)
      ? maximum
      : Number.POSITIVE_INFINITY;
    return Math.min(effectiveMaximum, Math.max(effectiveMinimum, requested));
  };

  if (presentation?.collapsible && (collapsed || activities.length === 0)) {
    return null;
  }

  const active =
    activities.find((activity) => activity.key === activeKey) ?? activities[0];
  const usesSectionNavigation = Boolean(
    active?.surfaces.some((surface) =>
      surface.contribution.traits.includes("activity-section"),
    ),
  );
  const requestedSurface = active
    ? selectedSurfaceByActivity[active.key]
    : undefined;
  const selectedSurface = active
    ? (active.surfaces.find(
        (surface) => activitySurfaceKey(surface) === requestedSurface,
      ) ?? active.surfaces[0])
    : undefined;

  return (
    <section
      className="rintawa-workbench-region"
      data-shell-region={region}
      data-region-mode={presentation?.mode ?? "plain"}
    >
      {presentation?.mode === "activity-tabs" && tabActivities.length > 0 ? (
        <div
          className="rintawa-workbench-tabs"
          role="tablist"
          aria-label={presentation.label}
        >
          <div className="rintawa-workbench-tab-strip">
            {tabActivities.map((activity) => (
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
        {active?.id === MANAGEMENT_ACTIVITY_ID ? (
          <header className="rintawa-management-header" data-world-mode={managementContext.world_id !== null}>
            {managementContext.world_id ? (
              <button
                type="button"
                className="rintawa-management-back"
                aria-label="Back to global Manage"
                title="Back to global Manage"
                onClick={onExitWorldManagement}
              >
                <span aria-hidden>←</span>
              </button>
            ) : null}
            <strong className="rintawa-management-title">
              {managementContext.world_id ? managementContext.label : "Manage"}
            </strong>
          </header>
        ) : null}
        {active ? (
          usesSectionNavigation ? (
            <div className="rintawa-activity-section-layout">
              <nav
                ref={sectionNavigationRef}
                className="rintawa-activity-section-nav"
                aria-label={`${active.label} sections`}
                style={
                  sectionNavigationWidth === undefined
                    ? undefined
                    : { flexBasis: `${sectionNavigationWidth}px` }
                }
              >
                {active.surfaces.map((surface) => {
                  const surfaceKey = activitySurfaceKey(surface);
                  const isSelected =
                    selectedSurface &&
                    activitySurfaceKey(selectedSurface) === surfaceKey;
                  return (
                    <button
                      key={surfaceKey}
                      type="button"
                      className="rintawa-activity-section-button"
                      data-active={isSelected}
                      aria-current={isSelected ? "page" : undefined}
                      onClick={() =>
                        setSelectedSurfaceByActivity((current) => ({
                          ...current,
                          [active.key]: surfaceKey,
                        }))
                      }
                    >
                      {humanizeSurface(surface)}
                    </button>
                  );
                })}
              </nav>
              <button
                type="button"
                className="rintawa-split-resize-handle rintawa-activity-section-resize-handle"
                data-axis="horizontal"
                role="separator"
                aria-orientation="vertical"
                aria-label={`Resize ${active.label} section navigation`}
                tabIndex={0}
                onPointerDown={(event) => {
                  const navigation = sectionNavigationRef.current;
                  if (!navigation) return;
                  event.preventDefault();
                  event.stopPropagation();
                  const startWidth = navigation.getBoundingClientRect().width;
                  beginWindowPointerResize("horizontal", event, (delta) => {
                    onResizeSectionNavigation(
                      active.key,
                      boundedSectionNavigationWidth(startWidth + delta),
                    );
                  });
                }}
                onKeyDown={(event) => {
                  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                  const navigation = sectionNavigationRef.current;
                  if (!navigation) return;
                  event.preventDefault();
                  const direction = event.key === "ArrowLeft" ? -1 : 1;
                  onResizeSectionNavigation(
                    active.key,
                    boundedSectionNavigationWidth(
                      navigation.getBoundingClientRect().width + direction * 24,
                    ),
                  );
                }}
              />
              <div className="rintawa-activity-section-content">
                {selectedSurface ? (
                  <PortableSurface
                    key={`${selectedSurface.owner.instance_id}:${selectedSurface.snapshot.surface_id}`}
                    surface={selectedSurface}
                    onAction={onAction}
                  />
                ) : null}
              </div>
            </div>
          ) : (
            active.surfaces.map((surface) => (
              <PortableSurface
                key={`${surface.owner.instance_id}:${surface.snapshot.surface_id}`}
                surface={surface}
                onAction={onAction}
              />
            ))
          )
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
  sectionNavigationWidths: Readonly<Record<string, number>>,
  onFocus: (activity: DerivedActivity) => void,
  onToggleCollapsed: (region: string) => void,
  onResizeSplit: (splitId: string, weights: number[]) => void,
  onResizeSectionNavigation: (activityKey: string, width: number) => void,
  onOpenMenu: (activity: DerivedActivity, anchor: HTMLElement) => void,
  onAction: (event: UiActionEvent) => void,
  showGlobalActivityTabs: boolean,
  managementContext: UiManagementContext,
  onExitWorldManagement: () => void,
  key: string,
): ReactNode {
  if (node.type === "region") {
    const presentation = shellRegionPresentation(pack, node);
    const regionActivities = assigned.get(node.region) ?? [];
    if (
      presentation?.collapsible &&
      (collapsed[node.region] || regionActivities.length === 0)
    ) {
      return null;
    }
    const tabActivities = showGlobalActivityTabs
      ? regionActivities
      : regionActivities.filter((activity) =>
          activity.surfaces.some(
            (surface) => surface.context?.kind === "focused-world",
          ),
        );
    return (
      <ActivityRegion
        key={key}
        region={node.region}
        presentation={presentation}
        activities={regionActivities}
        tabActivities={tabActivities}
        activeKey={activeByRegion[node.region]}
        collapsed={collapsed[node.region] ?? false}
        sectionNavigationWidth={
          sectionNavigationWidths[
            (regionActivities.find((activity) => activity.key === activeByRegion[node.region]) ??
              regionActivities[0])?.key ?? ""
          ]
        }
        onFocus={onFocus}
        onToggleCollapsed={() => onToggleCollapsed(node.region)}
        onResizeSectionNavigation={onResizeSectionNavigation}
        onOpenMenu={onOpenMenu}
        onAction={onAction}
        managementContext={managementContext}
        onExitWorldManagement={onExitWorldManagement}
      />
    );
  }

  const splitId = shellLayoutIdentity(node);
  const defaultWeights = node.children.map((child) =>
    child.type === "region" ? (child.weight ?? 1) : 1,
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
        sectionNavigationWidths,
        onFocus,
        onToggleCollapsed,
        onResizeSplit,
        onResizeSectionNavigation,
        onOpenMenu,
        onAction,
        showGlobalActivityTabs,
        managementContext,
        onExitWorldManagement,
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

    const pairPixels = geometry.pairPixels;
    beginWindowPointerResize(node.axis, event, (delta) => {
      resizeVisiblePair(visibleIndex, delta, pairPixels);
    });
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
              const decrease =
                node.axis === "horizontal" ? "ArrowLeft" : "ArrowUp";
              const increase =
                node.axis === "horizontal" ? "ArrowRight" : "ArrowDown";
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
  presentation: UiLayerPresentationState;
  onAction: (event: UiActionEvent) => void;
  onManagementHome: () => void;
  experiencePack: WebExperiencePack;
}

export function WorkbenchComposer({
  surfaces,
  presentation,
  onAction,
  onManagementHome,
  experiencePack,
}: WorkbenchComposerProps) {
  const layerLocalActivities = useMemo(
    () =>
      deriveActivities(
        surfaces.filter((surface) => surface.context?.kind !== "focused-world"),
        experiencePack,
      ),
    [surfaces, experiencePack],
  );
  const globalActivities = useMemo(
    () => deriveGlobalActivities(surfaces, experiencePack),
    [surfaces, experiencePack],
  );
  const focusedWorldActivities = useMemo(
    () => deriveFocusedWorldActivities(surfaces, experiencePack),
    [surfaces, experiencePack],
  );
  const entryActivity = useMemo(
    () =>
      focusedWorldEntryActivity(
        surfaces,
        focusedWorldActivities,
        presentation,
        experiencePack,
      ),
    [surfaces, focusedWorldActivities, presentation, experiencePack],
  );
  const activities = useMemo(() => {
    if (
      !entryActivity ||
      focusedWorldActivities.some(
        (activity) => activity.key === entryActivity.key,
      )
    ) {
      return [...layerLocalActivities, ...focusedWorldActivities];
    }
    return [...layerLocalActivities, ...focusedWorldActivities, entryActivity];
  }, [layerLocalActivities, focusedWorldActivities, entryActivity]);
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
  const lastAutoFocusedWorldEntry = useRef<string | null>(null);
  const [managementContext, setManagementContext] = useState<UiManagementContext>(
    presentation.management_context,
  );
  const lastExternalManagementRevision = useRef<number>(
    presentation.management_context.revision,
  );
  const dispatchedManagementScopes = useRef(new Set<string>());

  useEffect(() => {
    setPreferences(readWorkspacePreferences(experiencePack));
    setMenu(null);
  }, [experiencePack]);

  useEffect(() => {
    writeWorkspacePreferences(experiencePack, preferences);
  }, [experiencePack, preferences]);

  useEffect(() => {
    const external = presentation.management_context;
    if (external.revision === lastExternalManagementRevision.current) return;
    lastExternalManagementRevision.current = external.revision;
    setManagementContext(external);
    if (external.world_id === null) return;
    const management = layerLocalActivities.find(
      (activity) => activity.id === MANAGEMENT_ACTIVITY_ID,
    );
    if (!management) return;
    setPreferences((current) => {
      const requested = current.placements[management.key];
      const region =
        requested && destinationIds.has(requested)
          ? requested
          : management.defaultRegion;
      return {
        ...current,
        activeByRegion: { ...current.activeByRegion, [region]: management.key },
        collapsed: { ...current.collapsed, [region]: false },
      };
    });
  }, [presentation.management_context, layerLocalActivities, destinationIds]);

  useEffect(() => {
    const management = layerLocalActivities.find(
      (activity) => activity.id === MANAGEMENT_ACTIVITY_ID,
    );
    if (!management) return;
    for (const surface of management.surfaces) {
      const action = managementContextAction(surface, managementContext.scope_id);
      if (!action) continue;
      const dispatchKey = `${action.owner_instance_id}:${action.surface_id}:${action.surface_revision}:${managementContext.scope_id}`;
      if (dispatchedManagementScopes.current.has(dispatchKey)) continue;
      dispatchedManagementScopes.current.add(dispatchKey);
      onAction(action);
    }
  }, [layerLocalActivities, managementContext.scope_id, onAction]);

  useEffect(() => {
    const focused = presentation.focused_world;
    if (!focused || !entryActivity) {
      lastAutoFocusedWorldEntry.current = null;
      return;
    }
    const identity = `${focused.world_id}:${focused.descriptor.entry_surface_id}`;
    if (lastAutoFocusedWorldEntry.current === identity) return;
    lastAutoFocusedWorldEntry.current = identity;
    setPreferences((current) => {
      const requested = current.placements[entryActivity.key];
      const region =
        requested && destinationIds.has(requested)
          ? requested
          : entryActivity.defaultRegion;
      return {
        ...current,
        activeByRegion: {
          ...current.activeByRegion,
          [region]: entryActivity.key,
        },
        collapsed: { ...current.collapsed, [region]: false },
      };
    });
  }, [presentation.focused_world, entryActivity, destinationIds]);

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
      if (
        !regionActivities.some((activity) => activity.key === active[region])
      ) {
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
      override && destinationIds.has(override)
        ? override
        : activity.defaultRegion;
    updatePreferences((current) => ({
      ...current,
      activeByRegion: { ...current.activeByRegion, [region]: activity.key },
      collapsed: { ...current.collapsed, [region]: false },
    }));
  };

  const exitWorldManagement = () => {
    setManagementContext((current) => hostManagementContext(current.revision));
    onManagementHome();
  };

  const focusGlobalActivity = (activity: DerivedActivity) => {
    if (activity.id === MANAGEMENT_ACTIVITY_ID) {
      exitWorldManagement();
    }
    focusActivity(activity);
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
    if (!weights.every((weight) => Number.isFinite(weight) && weight > 0))
      return;
    updatePreferences((current) => ({
      ...current,
      splitWeights: {
        ...current.splitWeights,
        [splitId]: weights,
      },
    }));
  };

  const resizeSectionNavigation = (activityKey: string, width: number) => {
    if (!Number.isFinite(width) || width <= 0) return;
    updatePreferences((current) => ({
      ...current,
      sectionNavigationWidths: {
        ...current.sectionNavigationWidths,
        [activityKey]: width,
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
    preferences.sectionNavigationWidths,
    focusActivity,
    toggleCollapsed,
    resizeSplit,
    resizeSectionNavigation,
    openMenu,
    onAction,
    experiencePack.shell.activity_bar?.presentation === "hidden",
    managementContext,
    exitWorldManagement,
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
          {globalActivities.map((activity) => {
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
                onClick={() => focusGlobalActivity(activity)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  openMenu(activity, event.currentTarget);
                }}
              >
                <InterfaceIcon slot={activity.iconSlot} size={22} />
                <span className="rintawa-activity-label">{activity.label}</span>
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
