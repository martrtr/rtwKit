import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import ReactMarkdown from "react-markdown";

import type { UiActionEvent, UiPresentationSurface } from "../bridge";
import type { UiNode, UiTextInputNode } from "../bridge/types";
import { InterfaceIcon } from "../icons/InterfaceIcon";
import {
  EMPTY_PORTABLE_LAYOUT_OVERRIDES,
  effectiveChildOrder,
  effectiveWeights,
  portableLayoutStorageKey,
  readPortableLayoutOverrides,
  resizeAdjacentWeights,
  writePortableLayoutOverrides,
} from "./portableLayout";
import type { PortableLayoutOverrides } from "./portableLayout";

interface PortableSurfaceProps {
  surface: UiPresentationSurface;
  onAction: (event: UiActionEvent) => void;
}

interface NodeRendererProps extends PortableSurfaceProps {
  nodeId: string;
  nodes: ReadonlyMap<string, UiNode>;
  ancestors: ReadonlySet<string>;
}

interface PortableLayoutContextValue {
  overrides: PortableLayoutOverrides;
  setSplitWeights: (nodeId: string, weights: number[]) => void;
  setGridWeights: (nodeId: string, weights: number[]) => void;
}

const PortableLayoutContext = createContext<PortableLayoutContextValue>({
  overrides: EMPTY_PORTABLE_LAYOUT_OVERRIDES,
  setSplitWeights: () => {},
  setGridWeights: () => {},
});


interface NodePresentationAttributes {
  "data-ui-node-id": string;
  "data-ui-semantic"?: string;
  "data-ui-traits"?: string;
}

function nodePresentationAttributes(node: UiNode): NodePresentationAttributes {
  return {
    "data-ui-node-id": node.id,
    "data-ui-semantic": node.semantic
      ? `${node.semantic.id}@${node.semantic.version}`
      : undefined,
    "data-ui-traits":
      (node.traits?.length ?? 0) > 0 ? node.traits?.join(" ") : undefined,
  };
}

function actionEvent(
  surface: UiPresentationSurface,
  nodeId: string,
  actionId: string,
  value?: string | boolean,
): UiActionEvent {
  return {
    owner_instance_id: surface.owner.instance_id,
    surface_id: surface.snapshot.surface_id,
    node_id: nodeId,
    action_id: actionId,
    surface_revision: surface.snapshot.revision,
    payload:
      value === undefined
        ? { type: "none" }
        : typeof value === "boolean"
          ? { type: "boolean", value }
          : { type: "text", value },
  };
}

function TextControl({
  nodeId,
  data,
  surface,
  onAction,
  isMultiline,
  presentationAttributes,
}: {
  nodeId: string;
  data: UiTextInputNode;
  surface: UiPresentationSurface;
  onAction: (event: UiActionEvent) => void;
  isMultiline: boolean;
  presentationAttributes: NodePresentationAttributes;
}) {
  const [value, setValue] = useState(data.value);

  useEffect(() => setValue(data.value), [data.value]);

  const onChange = (nextValue: string) => {
    setValue(nextValue);
    if (data.change_action) {
      onAction(actionEvent(surface, nodeId, data.change_action, nextValue));
    }
  };

  const submit = () => {
    if (data.submit_action) {
      onAction(actionEvent(surface, nodeId, data.submit_action, value));
    }
  };

  if (isMultiline) {
    return (
      <>
        <textarea
          {...presentationAttributes}
          className="rintawa-input rintawa-textarea"
          value={value}
          placeholder={data.placeholder ?? undefined}
          disabled={!data.is_enabled}
          onChange={(event) => onChange(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              submit();
            }
          }}
        />
        {data.submit_action ? (
          <button
            type="button"
            className="rintawa-button rintawa-textarea-submit"
            data-appearance="default"
            disabled={!data.is_enabled}
            onClick={submit}
          >
            Submit
          </button>
        ) : null}
      </>
    );
  }

  return (
    <input
      {...presentationAttributes}
      className="rintawa-input"
      value={value}
      placeholder={data.placeholder ?? undefined}
      disabled={!data.is_enabled}
      onChange={(event) => onChange(event.currentTarget.value)}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          submit();
        }
      }}
    />
  );
}

function NodeRenderer({ surface, onAction, nodeId, nodes, ancestors }: NodeRendererProps) {
  const layout = useContext(PortableLayoutContext);
  const node = nodes.get(nodeId);
  if (!node) {
    return <div className="rintawa-node-error">Missing node: {nodeId}</div>;
  }
  if (ancestors.has(nodeId)) {
    return <div className="rintawa-node-error">Cyclic node: {nodeId}</div>;
  }

  const nextAncestors = new Set(ancestors).add(nodeId);
  const children = (ids: string[]) =>
    effectiveChildOrder(ids, layout.overrides.childOrder[node.id]).map((childId) => (
      <NodeRenderer
        key={childId}
        surface={surface}
        onAction={onAction}
        nodeId={childId}
        nodes={nodes}
        ancestors={nextAncestors}
      />
    ));

  const kind = node.kind;
  const presentationAttributes = nodePresentationAttributes(node);
  switch (kind.type) {
    case "text":
      return <span {...presentationAttributes} className="rintawa-text">{kind.data.text}</span>;
    case "markdown":
      return (
        <div {...presentationAttributes} className="rintawa-markdown">
          <ReactMarkdown
            components={{
              img: ({ alt }) => (
                <span className="rintawa-markdown-image-placeholder">
                  {alt ? `[Image: ${alt}]` : "[External image blocked]"}
                </span>
              ),
              a: ({ href, children }) => {
                let safeHref: string | undefined;
                try {
                  const parsed = href ? new URL(href) : null;
                  if (parsed?.protocol === "https:") safeHref = parsed.toString();
                } catch {
                  safeHref = undefined;
                }
                return safeHref ? (
                  <a
                    href={safeHref}
                    target="_blank"
                    rel="noreferrer noopener"
                  >
                    {children}
                  </a>
                ) : (
                  <span>{children}</span>
                );
              },
            }}
          >
            {kind.data.source}
          </ReactMarkdown>
        </div>
      );
    case "button":
      return (
        <button
          {...presentationAttributes}
          className="rintawa-button"
          data-appearance={kind.data.appearance}
          disabled={!kind.data.is_enabled}
          onClick={() => onAction(actionEvent(surface, node.id, kind.data.action))}
        >
          {kind.data.label}
        </button>
      );
    case "icon":
      return (
        <span
          {...presentationAttributes}
          className="rintawa-icon-node"
          title={kind.data.label ?? undefined}
          aria-label={kind.data.label ?? undefined}
        >
          <InterfaceIcon slot={kind.data.slot} size={kind.data.size ?? 20} />
        </span>
      );
    case "image":
      return (
        <img
          {...presentationAttributes}
          className="rintawa-image"
          src={`data:${kind.data.media_type};base64,${kind.data.data_base64}`}
          alt={kind.data.alt}
          width={kind.data.width ?? undefined}
          height={kind.data.height ?? undefined}
          loading="lazy"
          decoding="async"
        />
      );
    case "checkbox":
      return (
        <label {...presentationAttributes} className="rintawa-checkbox">
          <input
            type="checkbox"
            checked={kind.data.checked}
            disabled={!kind.data.is_enabled}
            onChange={(event) =>
              onAction(
                actionEvent(
                  surface,
                  node.id,
                  kind.data.change_action,
                  event.currentTarget.checked,
                ),
              )
            }
          />
          <span>{kind.data.label}</span>
        </label>
      );
    case "select":
      return (
        <select
          {...presentationAttributes}
          className="rintawa-input rintawa-select"
          value={kind.data.value}
          disabled={!kind.data.is_enabled}
          onChange={(event) =>
            onAction(
              actionEvent(
                surface,
                node.id,
                kind.data.change_action,
                event.currentTarget.value,
              ),
            )
          }
        >
          {kind.data.options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      );
    case "text-input":
      return (
        <TextControl
          nodeId={node.id}
          data={kind.data}
          surface={surface}
          onAction={onAction}
          isMultiline={false}
          presentationAttributes={presentationAttributes}
        />
      );
    case "text-area":
      return (
        <TextControl
          nodeId={node.id}
          data={kind.data}
          surface={surface}
          onAction={onAction}
          isMultiline
          presentationAttributes={presentationAttributes}
        />
      );
    case "split": {
      const weights = effectiveWeights(
        kind.data.weights,
        layout.overrides.splitWeights[node.id],
      );
      const resizeByKeyboard = (index: number, delta: number) => {
        const pairWeight = (weights[index] ?? 1) + (weights[index + 1] ?? 1);
        const minimumWeight = Math.max(pairWeight * 0.08, 0.1);
        const next = [...weights];
        const first = Math.min(
          pairWeight - minimumWeight,
          Math.max(minimumWeight, (weights[index] ?? 1) + delta),
        );
        next[index] = first;
        next[index + 1] = pairWeight - first;
        layout.setSplitWeights(node.id, next);
      };

      const beginResize = (
        event: ReactPointerEvent<HTMLButtonElement>,
        index: number,
      ) => {
        event.preventDefault();
        event.stopPropagation();
        const split = event.currentTarget.parentElement;
        if (!split) return;
        const panes = [...split.children].filter((element) =>
          element.classList.contains("rintawa-split-pane"),
        );
        const first = panes[index]?.getBoundingClientRect();
        const second = panes[index + 1]?.getBoundingClientRect();
        if (!first || !second) return;

        const horizontal = kind.data.axis === "horizontal";
        const startCoordinate = horizontal ? event.clientX : event.clientY;
        const pairPixels = horizontal
          ? first.width + second.width
          : first.height + second.height;
        const startWeights = [...weights];

        const move = (pointerEvent: PointerEvent) => {
          const coordinate = horizontal
            ? pointerEvent.clientX
            : pointerEvent.clientY;
          layout.setSplitWeights(
            node.id,
            resizeAdjacentWeights(
              startWeights,
              index,
              coordinate - startCoordinate,
              pairPixels,
              72,
            ),
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
        <div
          {...presentationAttributes}
          className="rintawa-split"
          data-axis={kind.data.axis}
        >
          {kind.data.children.flatMap((childId, index) => {
            const pane = (
              <div
                key={childId}
                className="rintawa-split-pane"
                style={{ flexGrow: weights[index] ?? 1 }}
              >
                <NodeRenderer
                  surface={surface}
                  onAction={onAction}
                  nodeId={childId}
                  nodes={nodes}
                  ancestors={nextAncestors}
                />
              </div>
            );
            if (index === kind.data.children.length - 1) return [pane];
            return [
              pane,
              <button
                key={childId + ":resize"}
                type="button"
                className="rintawa-split-resize-handle"
                data-axis={kind.data.axis}
                aria-label={`Resize panes ${index + 1} and ${index + 2}`}
                onPointerDown={(event) => beginResize(event, index)}
                onKeyDown={(event) => {
                  const decrease =
                    kind.data.axis === "horizontal" ? "ArrowLeft" : "ArrowUp";
                  const increase =
                    kind.data.axis === "horizontal" ? "ArrowRight" : "ArrowDown";
                  if (event.key === decrease) {
                    event.preventDefault();
                    resizeByKeyboard(index, -0.5);
                  } else if (event.key === increase) {
                    event.preventDefault();
                    resizeByKeyboard(index, 0.5);
                  }
                }}
              />,
            ];
          })}
        </div>
      );
    }
    case "row":
      return <div {...presentationAttributes} className="rintawa-row">{children(kind.data.children)}</div>;
    case "column":
      return <div {...presentationAttributes} className="rintawa-column">{children(kind.data.children)}</div>;
    case "list":
      return <div {...presentationAttributes} className="rintawa-list">{children(kind.data.children)}</div>;
    case "data-grid": {
      const defaultWeights = kind.data.columns.map((column) => column.weight);
      const weights = effectiveWeights(
        defaultWeights,
        layout.overrides.gridWeights[node.id],
      );
      const template = weights
        .map((weight) => "minmax(0, " + weight + "fr)")
        .join(" ");
      const rowCount = kind.data.cells.length / kind.data.columns.length;

      const beginColumnResize = (
        event: ReactPointerEvent<HTMLButtonElement>,
        index: number,
      ) => {
        event.preventDefault();
        event.stopPropagation();
        const heading = event.currentTarget.parentElement;
        const header = heading?.parentElement;
        if (!header) return;
        const headings = [...header.children].filter((element) =>
          element.classList.contains("rintawa-data-grid-heading"),
        );
        const first = headings[index]?.getBoundingClientRect();
        const second = headings[index + 1]?.getBoundingClientRect();
        if (!first || !second) return;

        const startX = event.clientX;
        const pairPixels = first.width + second.width;
        const startWeights = [...weights];
        const move = (pointerEvent: PointerEvent) => {
          layout.setGridWeights(
            node.id,
            resizeAdjacentWeights(
              startWeights,
              index,
              pointerEvent.clientX - startX,
              pairPixels,
              56,
            ),
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

      const resizeColumnByKeyboard = (index: number, delta: number) => {
        const pairWeight = (weights[index] ?? 1) + (weights[index + 1] ?? 1);
        const next = [...weights];
        const minimumWeight = Math.max(pairWeight * 0.08, 0.1);
        const left = Math.min(
          pairWeight - minimumWeight,
          Math.max(minimumWeight, (weights[index] ?? 1) + delta),
        );
        next[index] = left;
        next[index + 1] = pairWeight - left;
        layout.setGridWeights(node.id, next);
      };

      return (
        <div {...presentationAttributes} className="rintawa-data-grid" role="table">
          <div
            className="rintawa-data-grid-header"
            role="row"
            style={{ gridTemplateColumns: template }}
          >
            {kind.data.columns.map((column, index) => {
              const sortable = Boolean(column.sort_action && column.key);
              const sortDirection = column.sort_direction ?? null;
              return (
                <div
                  key={(column.key ?? column.label) + ":" + index}
                  className="rintawa-data-grid-heading"
                  role="columnheader"
                  aria-sort={sortable ? (sortDirection ?? "none") : undefined}
                  data-sortable={sortable}
                >
                  {sortable ? (
                    <button
                      type="button"
                      className="rintawa-data-grid-sort-button"
                      onClick={() =>
                        onAction(
                          actionEvent(
                            surface,
                            node.id,
                            column.sort_action!,
                            column.key!,
                          ),
                        )
                      }
                    >
                      <span>{column.label}</span>
                      {sortDirection ? (
                        <span aria-hidden="true">
                          {sortDirection === "ascending" ? "↑" : "↓"}
                        </span>
                      ) : null}
                    </button>
                  ) : (
                    column.label
                  )}
                  {index < kind.data.columns.length - 1 ? (
                    <button
                      type="button"
                      className="rintawa-data-grid-column-resize"
                      aria-label={`Resize ${column.label} column`}
                      onPointerDown={(event) => beginColumnResize(event, index)}
                      onKeyDown={(event) => {
                        if (event.key === "ArrowLeft") {
                          event.preventDefault();
                          resizeColumnByKeyboard(index, -0.5);
                        } else if (event.key === "ArrowRight") {
                          event.preventDefault();
                          resizeColumnByKeyboard(index, 0.5);
                        }
                      }}
                    />
                  ) : null}
                </div>
              );
            })}
          </div>
          {Array.from({ length: rowCount }, (_, rowIndex) => {
            const isSelected = kind.data.selected_rows.includes(rowIndex);
            const rowKey = kind.data.row_keys?.[rowIndex];
            const rowAction = kind.data.row_action ?? null;
            const isActionable = Boolean(rowKey && rowAction);
            const activateRow = () => {
              if (rowKey && rowAction) {
                onAction(actionEvent(surface, node.id, rowAction, rowKey));
              }
            };
            return (
              <div
                key={rowKey ?? rowIndex}
                className="rintawa-data-grid-row"
                role="row"
                aria-selected={isSelected}
                data-selected={isSelected}
                data-actionable={isActionable}
                data-row-key={rowKey}
                tabIndex={isActionable ? 0 : undefined}
                onClick={(event) => {
                  if (!isActionable) return;
                  const target = event.target;
                  if (
                    target instanceof Element &&
                    target.closest("button, input, select, textarea, a, [role=button]")
                  ) {
                    return;
                  }
                  activateRow();
                }}
                onKeyDown={(event) => {
                  if (!isActionable || event.target !== event.currentTarget) return;
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    activateRow();
                  }
                }}
                style={{ gridTemplateColumns: template }}
              >
                {kind.data.columns.map((_, columnIndex) => {
                  const cellIndex =
                    rowIndex * kind.data.columns.length + columnIndex;
                  const cellId = kind.data.cells[cellIndex];
                  return (
                    <div
                      key={cellId ?? rowIndex + ":" + columnIndex}
                      className="rintawa-data-grid-cell"
                      role="cell"
                      data-column-label={kind.data.columns[columnIndex]?.label}
                    >
                      {cellId ? (
                        <NodeRenderer
                          surface={surface}
                          onAction={onAction}
                          nodeId={cellId}
                          nodes={nodes}
                          ancestors={nextAncestors}
                        />
                      ) : null}
                    </div>
                  );
                })}
              </div>
            );
          })}
        </div>
      );
    }  }
}

function PortableSurfaceBody({ surface, onAction }: PortableSurfaceProps) {
  const nodes = useMemo(
    () => new Map(surface.snapshot.nodes.map((node) => [node.id, node])),
    [surface.snapshot.nodes],
  );
  const storageKey = portableLayoutStorageKey(
    surface.owner.instance_id,
    surface.snapshot.surface_id,
  );
  const [overrides, setOverrides] = useState<PortableLayoutOverrides>(() =>
    readPortableLayoutOverrides(storageKey),
  );

  useEffect(
    () => writePortableLayoutOverrides(storageKey, overrides),
    [storageKey, overrides],
  );

  const layout = useMemo<PortableLayoutContextValue>(
    () => ({
      overrides,
      setSplitWeights: (nodeId, weights) =>
        setOverrides((current) => ({
          ...current,
          splitWeights: { ...current.splitWeights, [nodeId]: weights },
        })),
      setGridWeights: (nodeId, weights) =>
        setOverrides((current) => ({
          ...current,
          gridWeights: { ...current.gridWeights, [nodeId]: weights },
        })),
    }),
    [overrides],
  );

  return (
    <PortableLayoutContext.Provider value={layout}>
      <section
        className="rintawa-surface"
        data-placement={surface.contribution.placement}
        data-surface-id={surface.snapshot.surface_id}
      >
        <NodeRenderer
          surface={surface}
          onAction={onAction}
          nodeId={surface.snapshot.root}
          nodes={nodes}
          ancestors={new Set()}
        />
      </section>
    </PortableLayoutContext.Provider>
  );
}

export function PortableSurface({ surface, onAction }: PortableSurfaceProps) {
  const storageKey = portableLayoutStorageKey(
    surface.owner.instance_id,
    surface.snapshot.surface_id,
  );
  return (
    <PortableSurfaceBody
      key={storageKey}
      surface={surface}
      onAction={onAction}
    />
  );
}
