import { describe, expect, test } from "vitest";

import { shouldSubmitMultilineGesture } from "../src/renderer/textControlSemantics";

const enter = {
  key: "Enter",
  isComposing: false,
  shiftKey: false,
  ctrlKey: false,
  metaKey: false,
};

describe("multiline text controls", () => {
  test("keeps plain Enter for a newline when submit-on-enter is absent", () => {
    expect(shouldSubmitMultilineGesture(enter, false)).toBe(false);
    expect(
      shouldSubmitMultilineGesture({ ...enter, shiftKey: true }, false),
    ).toBe(false);
  });

  test("submits renderer-owned multiline controls with Ctrl or Command Enter", () => {
    expect(
      shouldSubmitMultilineGesture({ ...enter, ctrlKey: true }, false),
    ).toBe(true);
    expect(
      shouldSubmitMultilineGesture({ ...enter, metaKey: true }, false),
    ).toBe(true);
  });

  test("never submits while an IME composition is active", () => {
    expect(
      shouldSubmitMultilineGesture(
        { ...enter, ctrlKey: true, isComposing: true },
        false,
      ),
    ).toBe(false);
  });
});
