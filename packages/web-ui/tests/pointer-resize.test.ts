import { describe, expect, test } from "vitest";

import {
  beginWindowPointerResize,
  pointerResizeCoordinate,
  type PointerResizeTarget,
} from "../src/renderer/pointerResize";

type Listener = (event: PointerEvent) => void;

class FakePointerTarget implements PointerResizeTarget {
  private readonly listeners = new Map<string, Set<Listener>>();

  addEventListener(type: string, listener: Listener): void {
    const bucket = this.listeners.get(type) ?? new Set<Listener>();
    bucket.add(listener);
    this.listeners.set(type, bucket);
  }

  removeEventListener(type: string, listener: Listener): void {
    this.listeners.get(type)?.delete(listener);
  }

  emit(type: string, clientX: number, clientY: number): void {
    const event = { clientX, clientY } as PointerEvent;
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

describe("pointer resize interaction", () => {
  test("tracks horizontal and vertical coordinates independently", () => {
    const event = { clientX: 40, clientY: 70 };
    expect(pointerResizeCoordinate("horizontal", event)).toBe(40);
    expect(pointerResizeCoordinate("vertical", event)).toBe(70);
  });

  test("continues outside the divider and stops on pointer release", () => {
    const target = new FakePointerTarget();
    const deltas: number[] = [];
    beginWindowPointerResize(
      "horizontal",
      { clientX: 100, clientY: 0 },
      (delta) => deltas.push(delta),
      target,
    );

    target.emit("pointermove", 132, 999);
    target.emit("pointermove", 84, -100);
    target.emit("pointerup", 84, -100);
    target.emit("pointermove", 200, 0);

    expect(deltas).toEqual([32, -16]);
  });
});
