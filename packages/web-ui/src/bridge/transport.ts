import type { HostToRendererMessage, RendererToHostMessage } from "./types";

export type TransportMessageHandler = (message: HostToRendererMessage) => void;
export type TransportErrorHandler = (error: Error) => void;
export type TransportDisconnect = () => void;

export interface UiTransport {
  connect(
    onMessage: TransportMessageHandler,
    onError: TransportErrorHandler,
  ): Promise<TransportDisconnect>;

  send(message: RendererToHostMessage): Promise<void>;
}
