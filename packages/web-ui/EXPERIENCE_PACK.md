# Web UI Experience Packs

The Web UI owns renderer-specific workspace policy. Portable Rintawa extensions do not
choose pixels, panes, DOM nodes, or CSS classes; they publish semantic UI surfaces plus
abstract placement hints.

Web UI 0.0.3 introduces the versioned declarative format:

```text
rintawa.web.experience-pack@1
```

An experience pack has two independent sections:

- `theme`: design tokens / renderer skin;
- `shell`: workspace topology and routing from semantic surfaces or portable placement
  hints into named Web regions.

This separation is intentional. A pack may change only colors, only workspace behavior,
or both.

## Standard pack

The built-in `rintawa.web.standard` pack preserves the current workbench behavior:

```text
horizontal split
  main
  sidebar

floating/status layers
  status
  dialog
  overlay
```

Portable hints are mapped to those regions declaratively rather than in React code.

## Partial overrides

A patch can extend the standard pack:

```json
{
  "format": "rintawa.web.experience-pack@1",
  "id": "example.purple",
  "name": "Purple",
  "extends": "rintawa.web.standard",
  "theme": {
    "tokens": {
      "ui.color.accent": "#c084fc"
    }
  }
}
```

Shell rules default to prepend semantics so a small semantic specialization can override
generic placement fallback without copying the whole standard rule set. Set
`rules_mode` to `replace` for a full routing-table replacement.

## Semantic routing

Rules may match a portable placement hint, a semantic contract, or both.

Specificity is deterministic:

```text
semantic + placement
        >
semantic
        >
placement
```

For example, a surface with `semantic = game.spell-circle@1` and
`placement = overlay` can be routed into a dedicated `spell-overlay` region by a
visual-novel pack while unrelated overlay surfaces keep the generic fallback.

Package identity is intentionally not a routing selector.

## Runtime selection seam

The standalone Web bundle defaults to `rintawa.web.standard`. A Web host/runtime may
inject a full pack or patch through the renderer-owned
`window.__RINTAWA_WEB_EXPERIENCE_PACK__` seam before React starts.

That seam is Web-specific configuration, not a Core API. Future package discovery and
selection must remain outside Rintawa Core and must not create a privileged theme or
Package Manager execution path.
