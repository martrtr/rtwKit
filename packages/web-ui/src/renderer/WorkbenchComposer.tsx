import { useEffect, useMemo, useState } from "react";
import type {
  CSSProperties,
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

export interface DerivedActivity {
  key: string;
  id: string;
  label: string;
  iconSlot: string | null;
  surfaces: UiPresentationSurface[];
  defaultRegion: string;
}

interface WorkspacePreferences {
  placements: Record<string, string>;
  activeByRegion: Record<string, string>;
  collapsed: Record<string, boolean>;
  sizes: Record<string, number>;
}

const EMPTY_PREFERENCES: WorkspacePreferences = {
  placements: {},
  activeByRegion: {},
  collapsed: {},
  sizes: {},
};

function workspaceStorageKey(pack: WebExperiencePack): string {
  return `rintawa.web.workspace.v1:${pack.id}`;
}

function readWorkspacePreferences(pack: WebExperiencePack): WorkspacePreferences {
  if (typeof window === "undefined") return EMPTY_PREFERENCES;
  try {
    const raw = window.localStorage.getItem(workspaceStorageKey(pack));
    if (!raw) return EMPTY_PREFERENCES;
    const parsed = JSON.parse(raw) as Partial<WorkspacePreferences>;
    return {
      placements: parsed.placements ?? {},
      activeByRegion: parsed.activeByRegion ?? {},
      collapsed: parsed.collapsed ?? {},
      sizes: parsed.sizes ?? {},
    };
  } catch {
    return EMPTY_PREFERENCES;
  }
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
  size: number | undefined;
  onFocus: (activity: DerivedActivity) => void;
  onToggleCollapsed: () => void;
  onResize: (size: number) => void;
  onOpenMenu: (activity: DerivedActivity, anchor: HTMLElement) => void;
  onAction: (event: UiActionEvent) => void;
  style?: CSSProperties;
}

function ActivityRegion({
  region,
  presentation,
  activities,
  activeKey,
  collapsed,
  size,
  onFocus,
  onToggleCollapsed,
  onResize,
  onOpenMenu,
  onAction,
  style,
}: ActivityRegionProps) {
  if (presentation?.collapsible && (collapsed || activities.length === 0)) {
    return null;
  }

  const active =
    activities.find((activity) => activity.key === activeKey) ?? activities[0];

  const resize = presentation?.resize;
  const beginResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (!resize) return;
    const regionElement = event.currentTarget.parentElement;
    if (!regionElement) return;

    event.preventDefault();
    const startX = event.clientX;
    const startSize = regionElement.getBoundingClientRect().width;
    const minimum = resize.min_size ?? 96;
    const maximum = resize.max_size ?? 4096;
    const direction = resize.edge === "end" ? 1 : -1;

    const move = (pointerEvent: PointerEvent) => {
      const requested =
        startSize + (pointerEvent.clientX - startX) * direction;
      onResize(Math.min(maximum, Math.max(minimum, requested)));
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
    <section
      className="rintawa-workbench-region"
      data-shell-region={region}
      data-region-mode={presentation?.mode ?? "plain"}
      style={style}
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

      {resize ? (
        <button
          type="button"
          className="rintawa-region-resize-handle"
          data-edge={resize.edge}
          aria-label={`Resize ${presentation.label}`}
          aria-valuenow={size === undefined ? undefined : Math.round(size)}
          onPointerDown={beginResize}
        />
      ) : null}
    </section>
  );
}

function renderWorkbenchLayout(
  node: ShellLayoutNode,
  pack: WebExperiencePack,
  assigned: ReadonlyMap<string, DerivedActivity[]>,
  activeByRegion: Readonly<Record<string, string>>,
  collapsed: Readonly<Record<string, boolean>>,
  sizes: Readonly<Record<string, number>>,
  onFocus: (activity: DerivedActivity) => void,
  onToggleCollapsed: (region: string) => void,
  onResizeRegion: (region: string, size: number) => void,
  onOpenMenu: (activity: DerivedActivity, anchor: HTMLElement) => void,
  onAction: (event: UiActionEvent) => void,
  key: string,
): ReactNode {
  if (node.type === "region") {
    const presentation = pack.shell.region_presentations?.find(
      (item) => item.region === node.region,
    );
    const requestedSize = presentation?.resize ? sizes[node.region] : undefined;
    const size =
      requestedSize !== undefined && Number.isFinite(requestedSize)
        ? Math.min(
            presentation?.resize?.max_size ?? 4096,
            Math.max(presentation?.resize?.min_size ?? 96, requestedSize),
          )
        : undefined;
    return (
      <ActivityRegion
        key={key}
        region={node.region}
        presentation={presentation}
        activities={assigned.get(node.region) ?? []}
        activeKey={activeByRegion[node.region]}
        collapsed={collapsed[node.region] ?? false}
        size={size}
        onFocus={onFocus}
        onToggleCollapsed={() => onToggleCollapsed(node.region)}
        onResize={(nextSize) => onResizeRegion(node.region, nextSize)}
        onOpenMenu={onOpenMenu}
        onAction={onAction}
        style={{
          flexGrow: size === undefined ? node.weight ?? 1 : 0,
          flexBasis: size === undefined ? 0 : `${size}px`,
        }}
      />
    );
  }

  const children = node.children
    .map((child, index) =>
      renderWorkbenchLayout(
        child,
        pack,
        assigned,
        activeByRegion,
        collapsed,
        sizes,
        onFocus,
        onToggleCollapsed,
        onResizeRegion,
        onOpenMenu,
        onAction,
        `${key}.${index}`,
      ),
    )
    .filter((child) => child !== null);

  if (children.length === 0) return null;
  if (children.length === 1) return children[0];

  return (
    <div key={key} className="rintawa-shell-split" data-axis={node.axis}>
      {children}
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
    if (typeof window === "undefined") return;
    window.localStorage.setItem(
      workspaceStorageKey(experiencePack),
      JSON.stringify(preferences),
    );
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

  const resizeRegion = (region: string, size: number) => {
    if (!Number.isFinite(size)) return;
    updatePreferences((current) => ({
      ...current,
      sizes: {
        ...current.sizes,
        [region]: size,
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
    preferences.sizes,
    focusActivity,
    toggleCollapsed,
    resizeRegion,
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
