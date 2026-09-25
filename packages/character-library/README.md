# Rintawa Character Library

Character Library is the recommended first-party package for reusable `rintawa.character-template@1` content. It lives in the separate rtwKit repository and owns CharacterTemplate semantics, bounded Tavern/Character Card V2 JSON+PNG compatibility, RTW content packing, content-handler validation, and runtime-neutral Character instantiation plans.

Rintawa Core remains feature-neutral: it provides generic content, asset, schema, world, and service mechanisms but contains no Character-specific implementation. The package now ships a thin Component Model content-handler component that provides the platform-owned validation service for `rintawa.character-template@1`; it does not receive privileged Core integration. Library UI and authoritative World materialization remain separate follow-up adapters over generic platform capabilities.
