import { parseHostMessage } from "./schema";
import type {
  TransportDisconnect,
  TransportErrorHandler,
  TransportMessageHandler,
  UiTransport,
} from "./transport";
import type { RendererToHostMessage } from "./types";

export interface InjectedUiHost {
  send(message: RendererToHostMessage): void | Promise<void>;
  subscribe(listener: (message: unknown) => void): TransportDisconnect;
}

declare global {
  interface Window {
    __RINTAWA_UI_HOST__?: InjectedUiHost;
  }
}

export class InjectedUiTransport implements UiTransport {
  constructor(private readonly host: InjectedUiHost) {}

  async connect(
    onMessage: TransportMessageHandler,
    onError: TransportErrorHandler,
  ): Promise<TransportDisconnect> {
    return this.host.subscribe((rawMessage) => {
      try {
        onMessage(parseHostMessage(rawMessage));
      } catch (error) {
        onError(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  async send(message: RendererToHostMessage): Promise<void> {
    await this.host.send(message);
  }
}
