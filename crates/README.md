# Reusable libraries

These libraries are retained from the experimental packages because they contain
useful tested domain logic independent from the retired runtime:

- `character-library`: bounded Tavern JSON/PNG decoding, metadata-preserving
  export, portrait binding and immutable CharacterTemplate RTW encoding.
- `package-manager`: registry validation, catalog merging and dependency solving.
- `inference-contracts`: bounded provider-neutral inference envelopes and codecs.
- `ai-provider`: OpenAI-compatible request/SSE parsing and model inventory parsing.

They are not installed extensions and do not claim browser or World integration.
Runtime adapters, package descriptors and product acceptance belong to the next
P10/P11 slices. Character card source-format compatibility is intentional import
behavior, not compatibility with the retired Core ABI.
