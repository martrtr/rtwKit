# P10 preparation: retire the experimental runtime

Status: implemented on 2026-11-22 within the owner's authorized cleanup scope. This is a
breaking source cleanup before implementing the accepted Rintawa D1–D8 client
contracts. It does not claim a working browser, Chat or AI product.

## Grounded baseline

rtwKit `7778272` still mirrors the retired guest ABI and pins Core `8293141`.
Portable UI, UI Layer, world projections, entity/facet commands and durable
EffectJobs no longer exist in the current Core. Chat additionally references a
machine-local `rintawa-mvp-cut-list` checkout. The existing web bridge, runtime
adapters, manifests and smoke scripts therefore cannot establish current MVP
behavior. `.vscode/` is tracked despite containing local architecture notes.

## Scope and ownership

- Remove runtime/UI adapters, descriptors, build scripts and tests coupled to
  those retired APIs, including their old WIT and dependency pins. Do not retain
  inactive adapters behind compatibility flags or publish placeholder RTWs.
- Keep useful existing libraries: bounded Tavern JSON/PNG parsing and portrait
  binding, CharacterTemplate RTW encoding, package catalog/dependency solving,
  and inference codecs/SSE/model parsing. Move these existing crates out of
  `packages/` into `crates/` so they cannot be mistaken for publishable extensions.
  Remove their Portable UI and retired World/content-handler coupling.
- Preserve RTW packing, independent releases and Pages registry tooling. Since
  old releases are explicitly being removed, accept only the current manifest
  schema 2 and registry schema 4; remove package-ID replacement compatibility.
  No migration or new registry schema is introduced.
- Mirror the accepted current Core WIT and pin its exact Git revision. Local
  path overrides belong to ignored developer configuration, not manifests.
- Ignore editor notes, local Cargo overrides, generated registry/site/smoke
  outputs and build caches. Untrack `.vscode/` while keeping its files locally.
- Remove the existing GitHub Releases in `martrtr/rtwKit`, as explicitly requested.
  Preserve source tags and future release/registry workflows. Record the release
  inventory and deletion results locally. A deployed Pages index must be rebuilt
  from the resulting release set; source edits alone do not update that deployment.

## Invariants and acceptance

No changes to Core, permissions, sandboxing, atomic storage or resource ownership.
No new transport or compatibility layer. Retained libraries perform no Host
activation or background work. Unsupported schemas fail rather than being coerced.

Check the retained workspace (fmt, clippy, tests, rustdoc and secret scans), current
WIT equality, repository metadata, empty-catalog generation and a real temporary
RTW build/registry roundtrip. Preserve parser security cases and dependency-solver
regressions. Scan tracked source for retired API imports, dead local paths and
removed build/test references. Verify ignored/private files stay on disk and out
of the Git index. Verify the remote release list after deletion.

## Acceptance evidence

The retained workspace's 44 tests, clippy, formatting, rustdoc and secret checks
passed. The Character/inference libraries also passed a wasm32 check. Publishing
acceptance includes a real temporary RTW build, generated registry output parsed
by the manager and consumed by its dependency solver, and an empty catalog with
verified repository icon bytes. All 12 old GitHub Releases were removed and the
remote list verified empty; source tags remain. The previously deployed Pages
index still needs the updated workflow to run after this source change is pushed.

## Next slice

Implement the accepted rtwKit browser bridge and shell against D8, then real
World/Character/Chat packages. Browser isolation, JS-only queryables, disconnect,
DockView and restart acceptance remain P10 work; Narrator/AI integration remains
P11. The library and publishing checks here do not substitute for that acceptance.
