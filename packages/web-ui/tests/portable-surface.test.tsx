import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";

import type { UiPresentationSurface } from "../src/bridge/types";
import { PortableSurface } from "../src/renderer/PortableSurface";

function dataGridSurface(selectedRows: number[]): UiPresentationSurface {
  return {
    owner: {
      instance_id: "demo.grid",
      component_id: "runtime",
    },
    contribution: {
      id: "grid",
      placement: "primary",
      semantic: null,
      activity: null,
      traits: [],
      required_capabilities: ["rintawa.ui.data-grid@1", "rintawa.ui.text@1"],
    },
    snapshot: {
      surface_id: "grid",
      revision: "1",
      root: "grid",
      nodes: [
        {
          id: "grid",
          semantic: { id: "example.items", version: 1 },
          traits: ["collection", "primary-content"],
          kind: {
            type: "data-grid",
            data: {
              columns: [
                {
                  key: "name",
                  label: "Name",
                  weight: 1,
                  sort_action: "grid.sort",
                  sort_direction: "ascending",
                },
              ],
              cells: ["first", "second"],
              selected_rows: selectedRows,
              row_keys: ["first-row", "second-row"],
              row_action: "grid.activate-row",
            },
          },
        },
        {
          id: "first",
          kind: { type: "text", data: { text: "First" } },
        },
        {
          id: "second",
          kind: { type: "text", data: { text: "Second" } },
        },
      ],
    },
  };
}

describe("portable surface", () => {
  test("marks feature-selected data-grid rows without owning selection state", () => {
    const markup = renderToStaticMarkup(
      <PortableSurface
        surface={dataGridSurface([1])}
        onAction={() => undefined}
      />,
    );

    expect(markup).toContain('aria-selected="false"');
    expect(markup).toContain('data-selected="false"');
    expect(markup).toContain('aria-selected="true"');
    expect(markup).toContain('data-selected="true"');
    expect(markup).toContain('aria-sort="ascending"');
    expect(markup).toContain('data-actionable="true"');
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain('data-row-key="first-row"');
    expect(markup).toContain('data-column-label="Name"');
    expect(markup).toContain('data-ui-node-id="grid"');
    expect(markup).toContain('data-ui-semantic="example.items@1"');
    expect(markup).toContain('data-ui-traits="collection primary-content"');
    expect(markup).toContain("First");
    expect(markup).toContain("Second");
  });
});
