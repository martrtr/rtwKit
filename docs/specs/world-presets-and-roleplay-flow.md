# Worlds as experiences: presets and character-first roleplay

Status: historical product proposal, superseded for implementation by Core
`docs/specs/mvp-story-delivery.md` and `docs/specs/world-template-packages.md`
under the owner direction of 2026-11-22. It is not the current delivery contract. The owner requested product philosophy before the next
architectural change. The five unwanted captions have been removed separately.

## Product model

A World is an independent saved experience. In the roleplay preset this is one
story, campaign or playthrough, usually presented as one primary conversation.
Two Worlds can use the same character card and different scenarios, settings or
extensions. They have independent World records and subsequent state; this does
not introduce shared mutable characters or memories across Worlds.

Characters belong to the World. A conversation selects its participants from that
World. The current implementation still supports one linked Character only.
Additional conversations can later represent parallel scenes or private exchanges
within the same experience. They are optional structure, not a required collection
every user must manage. Creating a new World is the normal route to a separate
story; creating another conversation is an advanced action inside an existing one.

A preset (user-facing term: World build) chooses an initial set of
extensions, their configuration and a starting UI. `Chat Roleplay` is the first
such experience. It does not make `roleplay` a permanent Core World type or add
a second classification system to World persistence.

## Proposed first-run flow

1. Create a World. When multiple installed presets exist, choose a preset such as
   `Chat Roleplay`. With only one available, no selection step is necessary.
2. The preset opens its starting UI directly. An empty roleplay World offers
   **Import character**, rather than an empty app catalog or a chat-management form.
3. Import commits the Character. The roleplay workflow then creates its initial
   conversation linked to that Character and opens it. Its name can default to
   the Character's display name without making that name an identity.
4. Subsequent entry resumes the current conversation, with explicit selection if
   it is missing or ambiguous. This is proposed UX, not current persisted state.
5. A button beside the conversation title reveals the conversation list only
   when needed. No permanent Discord-style channel sidebar in the default flow.
   A future opposite-side character drawer can provide participants and adding
   characters; the owner previously deferred that pane and multiple participants.

Imported Character and created conversation are distinct commits under the current
contracts. If the second step fails, keep the imported Character and show an
explicit action to finish creating/opening its conversation. An unknown outcome
must be reconciled by known IDs; no automatic replay of either mutation. Returning
to the starting screen must not manufacture another conversation for an existing
Character. Do not claim the two-step workflow is atomic or exactly-once.

## Presets versus distribution

Composition is the starting mechanism to reuse. A future modpack can deliver a
preset together with its RTWs and content. A folder is a possible delivery form,
not a new execution boundary. Installed bytes still use exact artifact selection,
and requested permissions remain subject to Host approval. A World materializes
its initial composition; editing a source collection must not silently alter
already-created Worlds. The initial choice
does not automatically trust everything a downloaded collection requests.

Avoid a new type registry, semantic Core UI rules, generic onboarding engine or
speculative pack format in this slice. Start with one coherent roleplay preset.
Adding a second installed experience later should exercise the same mechanisms.
Content-only datapacks and executable extension collections need not be collapsed
into one concept before an actual distribution requirement exists.

## Grounded implementation boundaries

- Core already materializes its default composition into a new World with fresh
  instance identities and copied provider/permission selections:
  `rintawa/crates/host/src/lib.rs`, `HostHome::create_world` and
  `CompositionProfile::materialize_all`. Profiles are a technical foundation,
  not an implemented browser preset catalog.
- The current browser `world-sessions.create` accepts no preset selector:
  [wire.rs](../../packages/web-runtime/src/wire.rs). Selecting arbitrary recipes
  needs a narrow creation contract; changing the machine-global default around
  one creation would race concurrent callers and is not an acceptable shortcut.
- Shell currently opens exact installed view identities:
  [App.tsx](../../packages/web-ui/src/App.tsx). There is no approved preset entry
  point. Do not infer authority or the starting component from a title like `Chat`.
- Character and Chat currently accept mutations only from their own instance's
  `ui`: [Character](../../packages/character/src/wasm.rs) and
  [Chat](../../packages/chat/src/wasm.rs). A new roleplay workflow cannot simply
  send mutations from an unrelated UI or expand Root into an arbitrary dispatcher.
- Current data has one optional `characterId` per conversation. Multiple
  participants, swipes, chat editing and AI orchestration remain separate work.

Before implementation, narrowly design and approve preset selection/entry and
owner-authorized import-to-conversation coordination. These are the consequential
boundaries suitable for an explicitly requested Astra analysis. A package merge,
broad mutation grant or new Core UI mechanism is not assumed approved.

## Acceptance for a subsequent implementation

- Two Worlds from the same preset remain independent after restart, including
  importing the same source card into both.
- An empty roleplay World opens the import action without manually choosing an app.
- A successful import opens one linked primary conversation. Failure or reply loss
  between commits preserves the Character and permits explicit recovery without
  duplicates or privilege widening.
- One conversation fills the workspace. Lists appear on explicit request; hiding
  them does not lose the selected conversation or draft.
- Ordinary user navigation does not require knowing extension/package names.
  Missing preset components produce a specific recovery state, not a silent fallback
  to an arbitrary similarly named provider.
