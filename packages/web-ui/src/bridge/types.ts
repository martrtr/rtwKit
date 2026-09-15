export const WEB_UI_BRIDGE_PROTOCOL_MAJOR = 1 as const;
export const PORTABLE_UI_PROTOCOL_MAJOR = 1 as const;

export const UI_CAPABILITIES = [
  "rintawa.ui.text@1",
  "rintawa.ui.markdown@1",
  "rintawa.ui.button@1",
  "rintawa.ui.input.text@1",
  "rintawa.ui.input.text-area@1",
  "rintawa.ui.layout.row@1",
  "rintawa.ui.layout.column@1",
  "rintawa.ui.list@1",
] as const;

export type UiCapability = (typeof UI_CAPABILITIES)[number];

export interface ComponentRef {
  instance_id: string;
  component_id: string;
}

export interface ContractKey {
  id: string;
  version: number;
}

export type UiPlacementHint =
  | "primary"
  | "secondary"
  | "sidebar"
  | "settings"
  | "dialog"
  | "status"
  | "overlay";

export interface UiSurfaceContribution {
  id: string;
  placement: UiPlacementHint;
  semantic: ContractKey | null;
  required_capabilities: string[];
}

export interface UiTextNode {
  text: string;
}

export interface UiMarkdownNode {
  source: string;
}

export interface UiButtonNode {
  label: string;
  action: string;
  is_enabled: boolean;
}

export interface UiTextInputNode {
  value: string;
  placeholder: string | null;
  change_action: string | null;
  submit_action: string | null;
  is_enabled: boolean;
}

export type UiTextAreaNode = UiTextInputNode;

export interface UiContainerNode {
  children: string[];
}

export type UiNodeKind =
  | { type: "text"; data: UiTextNode }
  | { type: "markdown"; data: UiMarkdownNode }
  | { type: "button"; data: UiButtonNode }
  | { type: "text-input"; data: UiTextInputNode }
  | { type: "text-area"; data: UiTextAreaNode }
  | { type: "row"; data: UiContainerNode }
  | { type: "column"; data: UiContainerNode }
  | { type: "list"; data: UiContainerNode };

export interface UiNode {
  id: string;
  kind: UiNodeKind;
}

export interface UiSurfaceSnapshot {
  surface_id: string;
  revision: string;
  root: string;
  nodes: UiNode[];
}

export interface UiPresentationSurface {
  owner: ComponentRef;
  contribution: UiSurfaceContribution;
  snapshot: UiSurfaceSnapshot;
}

export type UiActionPayload =
  | { type: "none" }
  | { type: "text"; value: string };

export interface UiActionEvent {
  owner_instance_id: string;
  surface_id: string;
  node_id: string;
  action_id: string;
  surface_revision: string;
  payload: UiActionPayload;
}

export interface RendererHelloMessage {
  type: "hello";
  protocol_major: typeof WEB_UI_BRIDGE_PROTOCOL_MAJOR;
  portable_ui_protocol_major: typeof PORTABLE_UI_PROTOCOL_MAJOR;
  capabilities: string[];
}

export interface RendererActionMessage {
  type: "action";
  protocol_major: typeof WEB_UI_BRIDGE_PROTOCOL_MAJOR;
  event: UiActionEvent;
}

export type RendererToHostMessage = RendererHelloMessage | RendererActionMessage;

export interface HostStateMessage {
  type: "state";
  protocol_major: typeof WEB_UI_BRIDGE_PROTOCOL_MAJOR;
  surfaces: UiPresentationSurface[];
}

export interface HostErrorMessage {
  type: "error";
  protocol_major: typeof WEB_UI_BRIDGE_PROTOCOL_MAJOR;
  code: string;
  message: string;
}

export type HostToRendererMessage = HostStateMessage | HostErrorMessage;
