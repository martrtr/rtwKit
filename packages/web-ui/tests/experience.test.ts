import { describe, expect, test } from "vitest";

import {
  STANDARD_EXPERIENCE_PACK,
  applyExperienceModule,
  mergeExperiencePack,
  parseExperiencePack,
  themeCssVariables,
} from "../src/experience";

describe("web experience packs", () => {
  test("loads the standard versioned renderer-specific format", () => {
    expect(STANDARD_EXPERIENCE_PACK.format).toBe("rintawa.web.experience-pack@1");
    expect(STANDARD_EXPERIENCE_PACK.id).toBe("rintawa.web.standard");
    expect(STANDARD_EXPERIENCE_PACK.shell.fallback_region).toBe("main");
    expect(STANDARD_EXPERIENCE_PACK.shell.activity_bar?.presentation).toBe("vertical-start");
    expect(STANDARD_EXPERIENCE_PACK.shell.workspace).toEqual({
      type: "region",
      region: "main",
      weight: 1,
    });
    expect(STANDARD_EXPERIENCE_PACK.shell.region_presentations?.map((item) => item.region)).toEqual([
      "main",
    ]);
    expect(
      STANDARD_EXPERIENCE_PACK.shell.rules.every(
        (rule) => !["left-dock", "right-dock"].includes(rule.region),
      ),
    ).toBe(true);
    expect(STANDARD_EXPERIENCE_PACK.theme.icons?.["activity.worlds"]).toBe("world");
    expect(STANDARD_EXPERIENCE_PACK.theme.icons?.["activity.management"]).toBe("tool");
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
    expect(
      themeCssVariables(STANDARD_EXPERIENCE_PACK)["--rintawa-web-section-nav-width"],
    ).toBe("12rem");
    expect(
      themeCssVariables(STANDARD_EXPERIENCE_PACK)["--rintawa-web-chat-message-radius"],
    ).toBe("0rem");
  });

  test("can theme flat chat articles into narrower classic message cards", () => {
    const custom = mergeExperiencePack(STANDARD_EXPERIENCE_PACK, {
      format: "rintawa.web.experience-pack@1",
      id: "demo.bubbles",
      name: "Bubbles",
      extends: "rintawa.web.standard",
      theme: {
        tokens: {
          "web.chat.message-max-width": "42rem",
          "web.chat.message-background": "#202126",
          "web.chat.message-local-background": "#252a38",
          "web.chat.message-border-width": "1px",
          "web.chat.message-radius": "1rem",
          "web.chat.message-spacing": "0.35rem",
          "web.chat.message-local-margin-left": "auto",
          "web.chat.message-local-margin-right": "0",
        },
      },
    });
    const variables = themeCssVariables(custom);
    expect(variables["--rintawa-web-chat-message-max-width"]).toBe("42rem");
    expect(variables["--rintawa-web-chat-message-background"]).toBe("#202126");
    expect(variables["--rintawa-web-chat-message-local-background"]).toBe("#252a38");
    expect(variables["--rintawa-web-chat-message-border-width"]).toBe("1px");
    expect(variables["--rintawa-web-chat-message-radius"]).toBe("1rem");
    expect(variables["--rintawa-web-chat-message-spacing"]).toBe("0.35rem");
    expect(variables["--rintawa-web-chat-message-local-margin-left"]).toBe("auto");
    expect(variables["--rintawa-web-chat-message-local-margin-right"]).toBe("0");
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

  test("applies ordered experience modules without changing base workspace identity", () => {
    const first = applyExperienceModule(STANDARD_EXPERIENCE_PACK, {
      format: "rintawa.web.experience-module@1",
      id: "demo.icons",
      name: "Demo Icons",
      targets: ["rintawa.web.standard"],
      theme: {
        tokens: { "ui.color.accent": "#111111" },
        icons: { "activity.extensions": "puzzle" },
      },
    });
    const second = applyExperienceModule(first, {
      format: "rintawa.web.experience-module@1",
      id: "demo.motion",
      name: "Demo Motion",
      theme: {
        tokens: {
          "ui.color.accent": "#222222",
          "web.motion.fast": "80ms",
        },
      },
    });

    expect(second.id).toBe(STANDARD_EXPERIENCE_PACK.id);
    expect(second.theme.tokens["ui.color.accent"]).toBe("#222222");
    expect(second.theme.tokens["web.motion.fast"]).toBe("80ms");
    expect(second.theme.icons?.["activity.extensions"]).toBe("puzzle");
  });

  test("rejects an experience module targeting another base pack", () => {
    expect(() =>
      applyExperienceModule(STANDARD_EXPERIENCE_PACK, {
        format: "rintawa.web.experience-module@1",
        id: "demo.foreign",
        name: "Foreign",
        targets: ["other.pack"],
        theme: { tokens: { "ui.color.accent": "#ffffff" } },
      }),
    ).toThrow("does not target base pack");
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
