import { DemoUiTransport } from "./demo";
import { InjectedUiTransport } from "./injected";
import type { UiTransport } from "./transport";
import { WebSocketUiTransport } from "./websocket";

export type { UiTransport } from "./transport";
export type {
  HostToRendererMessage,
  RendererToHostMessage,
  UiActionEvent,
  UiPresentationSurface,
} from "./types";
export { PORTABLE_UI_PROTOCOL_MAJOR, UI_CAPABILITIES, WEB_UI_BRIDGE_PROTOCOL_MAJOR } from "./types";

export function sameOriginWebSocketUrl(protocol: string, host: string): string | null {
  if (protocol !== "http:" && protocol !== "https:") {
    return null;
  }
  return `${protocol === "https:" ? "wss:" : "ws:"}//${host}/__rintawa/ws`;
}

export function createDefaultTransport(): UiTransport | null {
  if (window.__RINTAWA_UI_HOST__) {
    return new InjectedUiTransport(window.__RINTAWA_UI_HOST__);
  }

  const webSocketUrl = import.meta.env.VITE_RINTAWA_UI_WS_URL;
  if (typeof webSocketUrl === "string" && webSocketUrl.length > 0) {
    return new WebSocketUiTransport(webSocketUrl);
  }

  if (import.meta.env.DEV) {
    return new DemoUiTransport();
  }

  const sameOriginUrl = sameOriginWebSocketUrl(window.location.protocol, window.location.host);
  return sameOriginUrl ? new WebSocketUiTransport(sameOriginUrl) : null;
}
