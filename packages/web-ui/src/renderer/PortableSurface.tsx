import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";

import type { UiActionEvent, UiPresentationSurface } from "../bridge";
import type { UiNode, UiTextInputNode } from "../bridge/types";

interface PortableSurfaceProps {
  surface: UiPresentationSurface;
  onAction: (event: UiActionEvent) => void;
}

interface NodeRendererProps extends PortableSurfaceProps {
  nodeId: string;
  nodes: ReadonlyMap<string, UiNode>;
  ancestors: ReadonlySet<string>;
}

function actionEvent(
  surface: UiPresentationSurface,
  nodeId: string,
  actionId: string,
  value?: string,
): UiActionEvent {
  return {
    owner_instance_id: surface.owner.instance_id,
    surface_id: surface.snapshot.surface_id,
    node_id: nodeId,
    action_id: actionId,
    surface_revision: surface.snapshot.revision,
    payload: value === undefined ? { type: "none" } : { type: "text", value },
  };
}

function TextControl({
  nodeId,
  data,
  surface,
  onAction,
  isMultiline,
}: {
  nodeId: string;
  data: UiTextInputNode;
  surface: UiPresentationSurface;
  onAction: (event: UiActionEvent) => void;
  isMultiline: boolean;
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
      <textarea
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
    );
  }

  return (
    <input
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
  const node = nodes.get(nodeId);
  if (!node) {
    return <div className="rintawa-node-error">Missing node: {nodeId}</div>;
  }
  if (ancestors.has(nodeId)) {
    return <div className="rintawa-node-error">Cyclic node: {nodeId}</div>;
  }

  const nextAncestors = new Set(ancestors).add(nodeId);
  const children = (ids: string[]) =>
    ids.map((childId) => (
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
  switch (kind.type) {
    case "text":
      return <span className="rintawa-text">{kind.data.text}</span>;
    case "markdown":
      return (
        <div className="rintawa-markdown">
          <ReactMarkdown>{kind.data.source}</ReactMarkdown>
        </div>
      );
    case "button":
      return (
        <button
          className="rintawa-button"
          disabled={!kind.data.is_enabled}
          onClick={() => onAction(actionEvent(surface, node.id, kind.data.action))}
        >
          {kind.data.label}
        </button>
      );
    case "text-input":
      return (
        <TextControl
          nodeId={node.id}
          data={kind.data}
          surface={surface}
          onAction={onAction}
          isMultiline={false}
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
        />
      );
    case "row":
      return <div className="rintawa-row">{children(kind.data.children)}</div>;
    case "column":
      return <div className="rintawa-column">{children(kind.data.children)}</div>;
    case "list":
      return <div className="rintawa-list">{children(kind.data.children)}</div>;
  }
}

export function PortableSurface({ surface, onAction }: PortableSurfaceProps) {
  const nodes = useMemo(
    () => new Map(surface.snapshot.nodes.map((node) => [node.id, node])),
    [surface.snapshot.nodes],
  );

  return (
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
  );
}
