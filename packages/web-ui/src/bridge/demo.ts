import type {
  HostStateMessage,
  RendererToHostMessage,
  UiPresentationSurface,
} from "./types";
import { WEB_UI_BRIDGE_PROTOCOL_MAJOR } from "./types";
import type {
  TransportDisconnect,
  TransportErrorHandler,
  TransportMessageHandler,
  UiTransport,
} from "./transport";

function demoSurface(): UiPresentationSurface {
  return {
    owner: { instance_id: "demo.feature", component_id: "runtime" },
    contribution: {
      id: "demo.main",
      placement: "primary",
      semantic: null,
      required_capabilities: [],
    },
    snapshot: {
      surface_id: "demo.main",
      revision: "1",
      root: "root",
      nodes: [
        {
          id: "root",
          kind: { type: "column", data: { children: ["title", "body", "input", "notes", "row", "list"] } },
        },
        { id: "title", kind: { type: "text", data: { text: "Rintawa Web UI" } } },
        {
          id: "body",
          kind: {
            type: "markdown",
            data: { source: "Portable UI rendered by the **React Web Layer**." },
          },
        },
        {
          id: "input",
          kind: {
            type: "text-input",
            data: {
              value: "",
              placeholder: "Type and press Enter",
              change_action: null,
              submit_action: "demo.submit",
              is_enabled: true,
            },
          },
        },
        {
          id: "notes",
          kind: {
            type: "text-area",
            data: {
              value: "",
              placeholder: "Multiline input",
              change_action: "demo.notes",
              submit_action: null,
              is_enabled: true,
            },
          },
        },
        {
          id: "row",
          kind: { type: "row", data: { children: ["button", "status"] } },
        },
        {
          id: "button",
          kind: {
            type: "button",
            data: { label: "Run action", action: "demo.run", is_enabled: true },
          },
        },
        { id: "status", kind: { type: "text", data: { text: "Ready" } } },
        {
          id: "list",
          kind: { type: "list", data: { children: ["item-a", "item-b"] } },
        },
        { id: "item-a", kind: { type: "text", data: { text: "Text" } } },
        { id: "item-b", kind: { type: "text", data: { text: "Actions" } } },
      ],
    },
  };
}

export class DemoUiTransport implements UiTransport {
  private onMessage: TransportMessageHandler | null = null;
  private surface = demoSurface();

  async connect(
    onMessage: TransportMessageHandler,
    _onError: TransportErrorHandler,
  ): Promise<TransportDisconnect> {
    this.onMessage = onMessage;
    return () => {
      this.onMessage = null;
    };
  }

  async send(message: RendererToHostMessage): Promise<void> {
    if (message.type === "hello") {
      this.emitState();
      return;
    }

    const status = this.surface.snapshot.nodes.find((node) => node.id === "status");
    if (status?.kind.type === "text") {
      status.kind.data.text = `Action: ${message.event.action_id}`;
      this.surface.snapshot.revision = (
        BigInt(this.surface.snapshot.revision) + 1n
      ).toString();
    }
    this.emitState();
  }

  private emitState(): void {
    const message: HostStateMessage = {
      type: "state",
      protocol_major: WEB_UI_BRIDGE_PROTOCOL_MAJOR,
      surfaces: [structuredClone(this.surface)],
    };
    queueMicrotask(() => this.onMessage?.(message));
  }
}
