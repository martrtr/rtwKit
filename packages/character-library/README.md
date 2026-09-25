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
this through read-only composition metadata before creating anything. Core then
copies that exact immutable artifact and its granted component permissions into the new World
scope. This is composition policy, not hidden package privilege. File-picker
import/edit flows and PNG artwork extraction into `AssetRef` remain follow-up work.
