# Scenes

Scenes stores named contexts over arbitrary World items. Metadata belongs to the
scene; membership is an ordinary Scene→Item relation. Members are object items, as required by Core relation endpoints; relation-item
targets fail explicitly. Characters, locations, tools,
actions and other items retain their own owners and schemas. One item can belong
to multiple contexts. No actor ontology, inventory, transcript, observation log,
AI generation or separate database is included.

The editor discovers read-only catalogs from extensions. Characters and Atlas
provide initial catalogs; a third-party catalog can add any existing World items
without changing this package's metadata or Core. If a catalog disappears, existing
members stay visible with an ID fallback and remain removable.

## API

Selected contract `rintawa.scenes@2`; point `rintawa/scenes/v2`; change hints
`rintawa/scenes/changed/v2`. The experimental @1 utterance API is retired, as is its
Chat attachment path. Preview data is not automatically migrated or deleted.

Requests are strict JSON objects with a `method` field. Canonical UUID strings and
positive canonical u64 revision strings are required. Cursors must be present,
including explicit null. Responses use `{ok:true,value:...}` or
`{ok:false,error:{code:...}}`, within 64 KiB.

| Method | Arguments |
| --- | --- |
| `describe` | none |
| `allocate` | count, 1–3 |
| `list-scenes` | after: UUID or null; pages of 16 |
| `get-scene` | sceneId |
| `list-members` | sceneId, expectedRevision, after: UUID or null; pages of 32 |
| `item-scenes` | itemId, after: UUID or null; pages of 32 |
| `begin-create-scene` | sceneId, title, description |
| `begin-update-scene` | sceneId, expectedRevision, title, description |
| `begin-add-member` | sceneId, expectedRevision, itemId |
| `begin-remove-member` | sceneId, expectedRevision, itemId |
| `status` / `release` | operation |

Metadata is `{revision,title,description}`: title nonblank, <=256 UTF-8 bytes;
description <=4096. Member pages return
`{sceneId,revision,items:[{id,itemId}],next}`; incoming lookup returns
`{items:[{id,sceneId}],next}`. Relation ID ordering is pagination order, not time.
There is no total member-count cap. Discovery pages are not a global graph snapshot;
external item deletion can remove an incident membership. Actions must validate
their own current constraints when executed.

Only own UI and the exact selected Narrator backend can currently mutate/allocate;
other active same-World components may read this shared metadata. Catalogs gain
no write rights through discovery. Additional autonomous controllers require a
concrete admission decision; per-player privacy/control is not implied.

Mutations use activation-owned begin/status/release, 16 shared slots. A released
pending operation keeps its slot until terminal completion. Duplicate add returns
`already-member`, missing remove `not-member`, stale revision `stale-context`.
Removing membership never deletes its target. All scene-owned changes advance
revision atomically. Unknown outcomes are reconciled by rereading, never replayed
automatically. Existing World export carries the ordinary graph and properties.

## Catalog contributions

Use read-only multi-provider point `rintawa/scenes/catalogs/v1`:

- `describe` → `{catalogs:[{id,label,order}]}`, up to 8 per attested provider.
- `search {catalogId,query,after,provider}` → `{items:[{itemId,label,detail}],next}`.
  provider is `{instanceId,componentId,activation}` from Host-attested discovery,
  with activation encoded as a decimal string. It addresses a catalog, not authority.
  Other providers retire the request without a reply payload. The consumer checks
  the actual responding principal, never identity supplied in JSON.
- `describe-items {itemIds}` → `{items:[{itemId,label,detail}]}`, up to 32 requested
  IDs, for member display. Descriptions do not determine item identity or permission.

Catalog IDs are <=64 ASCII letters/digits/period/underscore/hyphen; descriptor labels
<=128 UTF-8 bytes; order is an integer within ±1000. Search query <=256 UTF-8 bytes;
result labels are nonblank <=256 bytes, detail <=512, pages <=32 rows, replies <=64
KiB. Search may return an empty page with an advancing cursor. Initial adapters
perform bounded local reads, no network or LLM calls. Extra response fields are
allowed; malformed/ambiguous replies cannot select a writer or route.

## Replacement and consumers

Implement the common contract and preserve stable scene/member IDs to replace
Scenes. The package ID, backend component name and internal storage are not the
interface. Existing Worlds require explicit retention/adoption of saved state;
installing another package cannot invent it. Keep contract selection and existing
authoritative point selection aligned when using targeted calls.

Narrator can use the stored membership as its candidate context without asking an
LLM to reconstruct the roster each turn. This package does not yet bind a Chat to
scenes or make Narrator consume them automatically. Current Chat already retains
its group roster separately. Membership does not prove perception or available
actions; those belong to the relevant mechanics and observation owners.
