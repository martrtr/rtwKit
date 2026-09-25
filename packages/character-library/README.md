# Rintawa Character Library

Character Library is the recommended first-party package for reusable
`rintawa.character-template@1` content. It lives in the separate rtwKit repository
and owns CharacterTemplate semantics, bounded Tavern/Character Card V2 JSON+PNG
compatibility, RTW content packing, content-handler validation, and runtime-neutral
Character instantiation plans.

Rintawa Core remains feature-neutral: it provides generic content, asset, schema,
world, and service mechanisms but contains no Character-specific implementation.
The package ships a permissionless Component Model content-handler for
`rintawa.character-template@1` plus a Portable UI/runtime component built only on
generic `user-content-read`, `world-session-write`, `world-command-submit`,
read-only `composition-read`, world schema registration, and the public World System
service protocol. The Library UI
browses the real persistent user-content index and can create/open a World and queue
an exact-revision Character instantiation command. The World-scoped instance of the
same package resolves that command into ordinary entity/facet/event proposals; Core
remains authoritative for validation and commit.

For the standard “Create World” flow, the exact Character Library activation must
be selected as a `world-default` before the World is created. The runtime verifies
this through read-only composition metadata before creating anything. Core then copies that
exact immutable artifact and its granted component permissions into the new World scope. This is
composition policy, not hidden package privilege.

Tavern PNG import now separates the embedded card metadata from sanitized portrait PNG bytes.
The portrait bytes can be published through the generic immutable asset store and are bound only
when the returned `AssetRef` matches the exact digest, size, and `image/png` media type. New live
Characters persist that optional reference in `rintawa.character.identity@2`; the original
`identity@1` schema remains unchanged for existing Worlds. Production file-picker/import/edit UX
still requires a generic deferred user-content write workflow rather than a Character-specific
Core shortcut.
