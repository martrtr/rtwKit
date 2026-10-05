export type PointerResizeAxis = "horizontal" | "vertical";

interface PointerPosition {
  clientX: number;
  clientY: number;
}

export interface PointerResizeTarget {
  addEventListener(
    type: "pointermove" | "pointerup" | "pointercancel",
    listener: (event: PointerEvent) => void,
  ): void;
  removeEventListener(
    type: "pointermove" | "pointerup" | "pointercancel",
    listener: (event: PointerEvent) => void,
  ): void;
}

export function pointerResizeCoordinate(
  axis: PointerResizeAxis,
  event: PointerPosition,
): number {
  return axis === "horizontal" ? event.clientX : event.clientY;
}

export function beginWindowPointerResize(
  axis: PointerResizeAxis,
  startEvent: PointerPosition,
  onDelta: (delta: number) => void,
  target: PointerResizeTarget = window,
): () => void {
  const startCoordinate = pointerResizeCoordinate(axis, startEvent);
  const move = (event: PointerEvent) => {
    onDelta(pointerResizeCoordinate(axis, event) - startCoordinate);
  };
  const stop = () => {
    target.removeEventListener("pointermove", move);
    target.removeEventListener("pointerup", stop);
    target.removeEventListener("pointercancel", stop);
  };
  target.addEventListener("pointermove", move);
  target.addEventListener("pointerup", stop);
  target.addEventListener("pointercancel", stop);
  return stop;
}
