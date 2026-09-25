# Rintawa Character Library

Character Library is the recommended first-party package for reusable `rintawa.character-template@1` content. It lives in the separate rtwKit repository and owns CharacterTemplate semantics, bounded Tavern/Character Card V2 JSON+PNG compatibility, RTW content packing, content-handler validation, and runtime-neutral Character instantiation plans.

Rintawa Core remains feature-neutral: it provides generic content, asset, schema, world, and service mechanisms but contains no Character-specific implementation. The package's thin runtime/content-handler WASM adapter is still pending, so release builds intentionally do not emit a placeholder RTW artifact until that adapter exists.
