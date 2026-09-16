import { useEffect, useMemo, useState } from "react";

import {
  PORTABLE_UI_PROTOCOL_MAJOR,
  UI_CAPABILITIES,
  WEB_UI_BRIDGE_PROTOCOL_MAJOR,
  createDefaultTransport,
} from "./bridge";
import type { UiActionEvent, UiPresentationSurface } from "./bridge";
import { SurfaceComposer } from "./renderer/SurfaceComposer";

export default function App() {
  const transport = useMemo(createDefaultTransport, []);
  const [surfaces, setSurfaces] = useState<UiPresentationSurface[]>([]);
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!transport) {
      setStatus("disconnected");
      return;
    }

    let isDisposed = false;
    let disconnect: (() => void) | undefined;

    void transport
      .connect(
        (message) => {
          if (isDisposed) return;
          if (message.type === "state") {
            setSurfaces(message.surfaces);
            setStatus("connected");
            setError(null);
          } else {
            setError(`${message.code}: ${message.message}`);
          }
        },
        (transportError) => {
          if (!isDisposed) {
            setStatus("error");
            setError(transportError.message);
          }
        },
      )
      .then(async (dispose) => {
        if (isDisposed) {
          dispose();
          return;
        }
        disconnect = dispose;
        await transport.send({
          type: "hello",
          protocol_major: WEB_UI_BRIDGE_PROTOCOL_MAJOR,
          portable_ui_protocol_major: PORTABLE_UI_PROTOCOL_MAJOR,
          capabilities: [...UI_CAPABILITIES],
        });
      })
      .catch((connectError: unknown) => {
        if (!isDisposed) {
          setStatus("error");
          setError(connectError instanceof Error ? connectError.message : String(connectError));
        }
      });

    return () => {
      isDisposed = true;
      disconnect?.();
    };
  }, [transport]);

  const dispatchAction = (event: UiActionEvent) => {
    if (!transport) return;
    void transport
      .send({
        type: "action",
        protocol_major: WEB_UI_BRIDGE_PROTOCOL_MAJOR,
        event,
      })
      .catch((actionError: unknown) => {
        setError(actionError instanceof Error ? actionError.message : String(actionError));
      });
  };

  return (
    <main className="rintawa-app">
      <header className="rintawa-header">
        <strong>Rintawa</strong>
        <span className="rintawa-status" data-status={status}>
          {status}
        </span>
      </header>

      {error ? <div className="rintawa-error">{error}</div> : null}

      {surfaces.length === 0 ? (
        <div className="rintawa-empty">No portable UI surfaces are mounted.</div>
      ) : (
        <SurfaceComposer surfaces={surfaces} onAction={dispatchAction} />
      )}
    </main>
  );
}
