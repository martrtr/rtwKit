import { readFile } from "node:fs/promises";

import { describe, expect, test } from "vitest";

const portableTokens = [
  "color-text-primary",
  "color-text-muted",
  "color-surface-primary",
  "color-surface-secondary",
  "color-accent",
  "color-warning",
  "color-error",
  "color-success",
  "spacing-compact",
  "spacing-normal",
  "spacing-relaxed",
  "radius-control",
  "font-body",
  "font-heading",
  "font-monospace",
] as const;

describe("web UI theme", () => {
  test("defines the portable semantic token vocabulary", async () => {
    const theme = await readFile(new URL("../src/theme.css", import.meta.url), "utf8");

    for (const token of portableTokens) {
      expect(theme).toContain(`--rintawa-ui-${token}:`);
    }
  });

  test("keeps the current web overrides as compatibility fallbacks", async () => {
    const theme = await readFile(new URL("../src/theme.css", import.meta.url), "utf8");

    expect(theme).toContain("var(--rintawa-color-text,");
    expect(theme).toContain("var(--rintawa-color-surface,");
    expect(theme).toContain("var(--rintawa-spacing-normal,");
    expect(theme).toContain("var(--rintawa-radius-control,");
  });

  test("renders through semantic tokens instead of legacy variables", async () => {
    const styles = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");

    expect(styles).toContain("var(--rintawa-ui-color-text-primary)");
    expect(styles).toContain("var(--rintawa-ui-color-surface-primary)");
    expect(styles).toContain("var(--rintawa-ui-spacing-normal)");
    expect(styles).not.toContain("var(--rintawa-color-text,");
    expect(styles).not.toContain("var(--rintawa-color-surface,");
  });
});
