# World Atlas

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
