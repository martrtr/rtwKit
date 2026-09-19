export const WEB_UI_BRIDGE_PROTOCOL_MAJOR = 1 as const;
export const PORTABLE_UI_PROTOCOL_MAJOR = 1 as const;

export const UI_CAPABILITIES = [
  "rintawa.ui.text@1",
  "rintawa.ui.markdown@1",
  "rintawa.ui.button@1",
  "rintawa.ui.icon@1",
  "rintawa.ui.image@1",
  "rintawa.ui.input.checkbox@1",
  "rintawa.ui.input.select@1",
  "rintawa.ui.input.text@1",
  "rintawa.ui.input.text-area@1",
  "rintawa.ui.layout.split@1",
  "rintawa.ui.layout.row@1",
  "rintawa.ui.layout.column@1",
  "rintawa.ui.list@1",
  "rintawa.ui.data-grid@1",
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

export interface UiActivityContribution {
  id: string;
  label: string;
  icon_slot: string | null;
}

export interface UiSurfaceContribution {
  id: string;
  placement: UiPlacementHint;
  semantic: ContractKey | null;
  activity: UiActivityContribution | null;
  traits: string[];
  required_capabilities: string[];
}

export interface UiTextNode {
  text: string;
}

export interface UiMarkdownNode {
  source: string;
}

export type UiButtonAppearance = "default" | "primary" | "subtle" | "danger";

export interface UiButtonNode {
  label: string;
  action: string;
  is_enabled: boolean;
  appearance: UiButtonAppearance;
}

export interface UiIconNode {
  slot: string;
  label: string | null;
  size: number | null;
}

export interface UiImageNode {
  media_type: string;
  data_base64: string;
  alt: string;
  width: number | null;
  height: number | null;
}

export interface UiCheckboxNode {
  label: string;
  checked: boolean;
  change_action: string;
  is_enabled: boolean;
}

export interface UiSelectOption {
  value: string;
  label: string;
}

export interface UiSelectNode {
  value: string;
  options: UiSelectOption[];
  change_action: string;
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

export interface UiSplitNode {
  children: string[];
  weights: number[];
  axis: "horizontal" | "vertical";
}

export interface UiDataGridColumn {
  key?: string | null;
  label: string;
  weight: number;
  sort_action?: string | null;
  sort_direction?: "ascending" | "descending" | null;
}

export interface UiDataGridNode {
  columns: UiDataGridColumn[];
  cells: string[];
  selected_rows: number[];
  row_keys?: string[];
  row_action?: string | null;
}

export type UiNodeKind =
  | { type: "text"; data: UiTextNode }
  | { type: "markdown"; data: UiMarkdownNode }
  | { type: "button"; data: UiButtonNode }
  | { type: "icon"; data: UiIconNode }
  | { type: "image"; data: UiImageNode }
  | { type: "checkbox"; data: UiCheckboxNode }
  | { type: "select"; data: UiSelectNode }
  | { type: "text-input"; data: UiTextInputNode }
  | { type: "text-area"; data: UiTextAreaNode }
  | { type: "split"; data: UiSplitNode }
  | { type: "row"; data: UiContainerNode }
  | { type: "column"; data: UiContainerNode }
  | { type: "list"; data: UiContainerNode }
  | { type: "data-grid"; data: UiDataGridNode };

export interface UiNode {
  id: string;
  semantic?: ContractKey | null;
  traits?: string[];
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
  | { type: "text"; value: string }
  | { type: "boolean"; value: boolean };

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
