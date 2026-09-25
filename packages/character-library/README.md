# Rintawa Character Library

Character Library is the recommended first-party package for reusable
`rintawa.character-template@1` content. It lives in the separate rtwKit repository
and owns CharacterTemplate semantics, bounded Tavern/Character Card V2 JSON+PNG
compatibility, RTW content packing, content-handler validation, and runtime-neutral
Character instantiation plans.

Rintawa Core remains feature-neutral: it provides generic content, asset, schema,
world, and service mechanisms but contains no Character-specific implementation.
The package ships a permissionless Component Model content-handler for
`rintawa.character-template@1` plus a separate Portable UI runtime that receives
only generic `user-content-read`. The Library UI browses the real persistent
user-content index and exact immutable descriptors; file-picker import/edit flows
and authoritative World materialization remain separate follow-up adapters over
generic platform capabilities.
