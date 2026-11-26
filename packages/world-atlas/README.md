# World Atlas

## Lorebook

The same package owns non-confidential authored lore as well as places/scenes.
Open **Lorebook** to create/edit notes, enable narration, choose always-relevant
or literal keywords/exclusions, priority and an optional conversation scope.
Notes start disabled. Preview uses saved notes and never calls AI. Disabled notes
remain editable; this first version supports 64 entries including disabled ones.

Own UI methods on `rintawa/world-atlas/v1`: `list-lore {after?}`,
`get-lore {itemId}`, `begin-create-lore {itemId,record}` and
`begin-update-lore {itemId,expectedRevision,record}`. Create uses revision `"1"`;
update submits the captured revision and the owner increments it. Existing
pending/status/release semantics apply. The new property is
`rintawa.world-atlas.lore@1`; new command `rintawa.world-atlas.lore-author@1`
leaves existing immutable location/scene schemas unchanged. Entries are portable.

`select-lore {conversationId,text}` admits only own UI or the exact selected
Narrator. It filters enabled/scope, then exclusions (including always-relevant
notes), then any keyword or always-relevant. Matching uses Unicode lowercase
literal substrings, without normalization. Sort is priority descending, ID
ascending; return <=8 whole entries and <=8 KiB JSON with explicit omissions.
There is no mutation, clock, embedding or inference in selection. Empty keyword
lists do not match unless always-relevant. Input text is <=16 KiB UTF-8.

Enabled notes are author-approved generation context, not proof of every actor's
knowledge. Private NPC knowledge and mechanical rule execution are outside this
slice. Imported Character books are not automatically copied here. Updated
Narrator and Atlas must be installed together when Atlas is selected; no fallback
to the retired API or silent omission on a failed selected provider is added.

Directed reads require an authoritative point pin as well as the preferred
contract provider. Build a complete optional template with
`python3 scripts/build-roleplay-template.py --artifacts <directory> --output <new.rtw> --world-atlas`.
It embeds exact Atlas bytes, requested grants and both pins. The default template
without that flag is unchanged. Existing Worlds need an explicit composition
update; installing a package alone does not grant it a transport point.

An optional World-scoped authoring app. Create locations, then scenes at one active
location; edit names, descriptions and archive flags; move scenes independently.
The app is not required by Chat. Install alongside Web Runtime.

The backend serves `rintawa/world-atlas/v1`. An admitted World component may call
`describe`, `list-locations`, `list-scenes`, `get-location`, and `get-scene`.
Only the app UI can allocate and mutate. Requests use the existing domain envelope.

Example: allocate three IDs over the UI in two calls, create the location, and
create a scene using a fresh relation ID. Wait on each operation and release it
before proceeding.

```json
{"method":"begin-create-location","itemId":"01900000-0000-7000-8000-000000000001","record":{"revision":"1","name":"Harbor","description":"A foggy dock.","archived":false}}
{"method":"begin-create-scene","itemId":"01900000-0000-7000-8000-000000000002","relationId":"01900000-0000-7000-8000-000000000003","locationId":"01900000-0000-7000-8000-000000000001","record":{"revision":"1","name":"Arrival","description":"Meet at the pier.","archived":false}}
{"method":"get-scene","itemId":"01900000-0000-7000-8000-000000000002"}
```

These are two objects with separate `rintawa.world-atlas.location@1` and
`rintawa.world-atlas.scene@1` properties, plus one directed Scene→Location relation
with the empty `rintawa.world-atlas.at@1` property. The scene DTO derives
`locationId` from that relation. Portability metadata declares all three
schemas. Use allocated IDs rather than the illustrative values above.
