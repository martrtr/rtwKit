# rtwKit architecture

## Role

rtwKit is the recommended first-party package set and package source for Rintawa.
It is not part of Rintawa Core and must not receive privileged runtime semantics
simply because it is shipped by default.

Rintawa Core provides mechanisms: RTW validation, immutable artifact storage,
runtime scopes, extension instances, permissions, services, Portable UI, and
activation primitives.

rtwKit provides policy and ecosystem behavior: package discovery, repositories,
version selection, dependency solving, updates, packs, developer sources, and
human-facing package-management UX.

The boundary is intentional: another package manager must be able to replace the
rtwKit Extension Manager without changing Core.

`packages/web-runtime` follows the same rule. It is an ordinary root-WASM
extension that publishes `rintawa.runtime.web-bundle@1`; HTTP, WebSocket and
browser bridge details remain outside Core. `packages/web-ui` is a separate
consumer of that execution target and provides the composition roles
`rintawa.ui.layer@1` and `rintawa.host.shell@1`.

`packages/world-manager` and `packages/character-library` follow the same boundary.
World Manager owns product-facing world catalog/lifecycle UX over the generic
`world-sessions` capability. Character Library owns CharacterTemplate/Tavern compatibility
and Character instantiation semantics over generic content/world contracts. Its baseline UI
uses generic deferred user-content writes plus exact `world-default` composition metadata and
world lifecycle/command access. New materialization commands are self-contained and bind the
embedded template to its canonical RTW revision before the inherited world-scoped instance
proposes ordinary entity/facet/event changes through the public World System contract. Neither
feature belongs in the Rintawa Core repository.

`wit/engine.wit` is a checked-in authoring mirror of the public Rintawa guest ABI used by
rtwKit packages. It must track the exact pinned Rintawa Git revision in `Cargo.toml`; rtwKit
must not invent package-only host capabilities.

## Monorepo boundary

`rtwKit` is a monorepo for first-party/recommended packages. Repository boundaries
must not be confused with package boundaries.

One repository may contain many independently versioned packages, and one package
may later move to another repository without changing its logical package id.
Packages under `packages/<slug>/` may be large applications (`web-ui`), small
providers, theme collections, or bundles. Size alone is not a reason to split a
repository. Split when ownership, community, release lifecycle, or branding
becomes genuinely independent.

GitHub Issues may be shared across the monorepo. Use package-scoped labels such as
`package:web-ui` and `package:extension-manager`, plus issue forms that ask for the
package and version. Do not create repositories merely to obtain separate issue
lists.

Third-party authors are free to use either one-package repositories or their own
monorepos. rtwKit is an example layout, not an ecosystem requirement.

## Identities

Keep these concepts distinct:

- package id: logical software identity, for example `rintawa.web-ui`;
- RTW artifact digest: immutable identity of exact bytes;
- extension id: logical runtime extension identity inside an RTW package;
- extension instance id: exact running instance;
- runtime scope id: where an instance runs;
- release tag: source/distribution version identity.

Never infer execution identity or permissions from a package id alone.## Core / manager split

Core must not know about GitHub, HTTP registries, `latest`, SemVer solving, Git
repositories, npm, package packs, or rtwKit-specific metadata.

The Extension Manager may use those concepts, but it must ultimately hand Core
exact artifacts and exact activation requests. Core never activates `latest` or a
version range.

Production package bytes are immutable RTW artifacts stored by digest. Updating a
package means importing a new artifact and changing composition/activation state;
it never mutates the old artifact in place.

Runtime scopes are independent from future State Engine worlds. Host/profile/world
are expected scopes, but the topology may later include workspaces, sessions, or
cross-world services without changing the RTW format.

## Anti-goals

Do not make rtwKit repository layout part of the Rintawa specification.
Do not make rtwKit registry JSON mandatory for Core.
Do not give rtwKit packages hidden capabilities unavailable to third parties.
Do not equate `WorldId`, package directories, runtime scopes, or installations.
Do not add special cases to Core for Chat, Web UI, AI providers, themes, or packs.
