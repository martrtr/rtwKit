# Narrator

World-scoped story MVP component. See ../../docs/specs/p10-narrator-ai.md.

Optional selected Psychology receives a best-effort after-response notification
only after the reply's append is known committed. At most 16 transient query
observers with a five-second deadline are held independently of generation results.
Notification/analysis never changes a saved reply's success or delays its terminal
result. Psychology owns subscription opt-in, processing and diagnostics; Narrator
has no psychological state, scheduler, notification retry or persistent job queue.
