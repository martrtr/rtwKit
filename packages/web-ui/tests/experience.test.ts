import { describe, expect, test } from "vitest";

import {
  STANDARD_EXPERIENCE_PACK,
  mergeExperiencePack,
  parseExperiencePack,
  themeCssVariables,
} from "../src/experience";

describe("web experience packs", () => {
  test("loads the standard versioned renderer-specific format", () => {
    expect(STANDARD_EXPERIENCE_PACK.format).toBe("rintawa.web.experience-pack@1");
    expect(STANDARD_EXPERIENCE_PACK.id).toBe("rintawa.web.standard");
    expect(STANDARD_EXPERIENCE_PACK.shell.fallback_region).toBe("main");
  });

  test("can override theme tokens without changing shell topology", () => {
    const custom = mergeExperiencePack(STANDARD_EXPERIENCE_PACK, {
      format: "rintawa.web.experience-pack@1",
      id: "demo.purple",
      name: "Purple",
      extends: "rintawa.web.standard",
      theme: {
        tokens: { "ui.color.accent": "#c084fc" },
      },
    });

    expect(custom.theme.tokens["ui.color.accent"]).toBe("#c084fc");
    expect(custom.shell.workspace).toEqual(STANDARD_EXPERIENCE_PACK.shell.workspace);
    expect(themeCssVariables(custom)["--rintawa-ui-color-accent"]).toBe("#c084fc");
  });

  test("can prepend a semantic layout override without copying default rules", () => {
    const custom = mergeExperiencePack(STANDARD_EXPERIENCE_PACK, {
      format: "rintawa.web.experience-pack@1",
      id: "demo.magic",
      name: "Magic",
      extends: "rintawa.web.standard",
      shell: {
        rules: [
          { match: { semantic: "game.spell-circle@1" }, region: "overlay" },
        ],
      },
    });

    expect(custom.shell.rules[0].match.semantic).toBe("game.spell-circle@1");
    expect(custom.shell.rules.length).toBe(STANDARD_EXPERIENCE_PACK.shell.rules.length + 1);
  });

  test("rejects rules that target undeclared regions", () => {
    expect(() =>
      parseExperiencePack({
        ...STANDARD_EXPERIENCE_PACK,
        id: "demo.invalid",
        shell: {
          ...STANDARD_EXPERIENCE_PACK.shell,
          rules: [{ match: { placement: "primary" }, region: "missing" }],
        },
      }),
    ).toThrow("unknown region");
  });
});
