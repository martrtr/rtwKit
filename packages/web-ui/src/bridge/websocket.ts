import { parseHostMessage } from "./schema";
import type {
  TransportDisconnect,
  TransportErrorHandler,
  TransportMessageHandler,
  UiTransport,
} from "./transport";
import type { RendererToHostMessage } from "./types";

export class WebSocketUiTransport implements UiTransport {
  private socket: WebSocket | null = null;

  constructor(private readonly url: string) {}

  async connect(
    onMessage: TransportMessageHandler,
    onError: TransportErrorHandler,
  ): Promise<TransportDisconnect> {
    if (this.socket) {
      throw new Error("WebSocket transport is already connected");
    }

    const socket = new WebSocket(this.url);
    this.socket = socket;

    try {
      await new Promise<void>((resolve, reject) => {
        const opened = () => {
          cleanup();
          resolve();
        };
        const failed = () => {
          cleanup();
          reject(new Error("WebSocket connection failed"));
        };
        const cleanup = () => {
          socket.removeEventListener("open", opened);
          socket.removeEventListener("error", failed);
        };
        socket.addEventListener("open", opened);
        socket.addEventListener("error", failed);
      });
    } catch (error) {
      if (this.socket === socket) {
        this.socket = null;
      }
      socket.close();
      throw error;
    }

    let isDisconnected = false;
    let hasTransportFailure = false;
    socket.addEventListener("message", (event) => {
      try {
        const rawMessage: unknown = JSON.parse(String(event.data));
        onMessage(parseHostMessage(rawMessage));
      } catch (error) {
        onError(error instanceof Error ? error : new Error(String(error)));
      }
    });
    socket.addEventListener("error", () => {
      hasTransportFailure = true;
      onError(new Error("WebSocket transport failed"));
    });
    socket.addEventListener("close", () => {
      if (this.socket === socket) {
        this.socket = null;
      }
      if (!isDisconnected && !hasTransportFailure) {
        onError(new Error("WebSocket transport disconnected"));
      }
    });

    return () => {
      isDisconnected = true;
      if (this.socket === socket) {
        this.socket = null;
      }
      socket.close();
    };
  }

  async send(message: RendererToHostMessage): Promise<void> {
    if (this.socket?.readyState !== WebSocket.OPEN) {
      throw new Error("WebSocket transport is not connected");
    }
    this.socket.send(JSON.stringify(message));
  }
}
