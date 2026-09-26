# Chat

Standard persistent Chat package for Rintawa. It owns versioned conversation, participant, message, revision, branching, event, projection, and Portable UI semantics while using only public World and runtime contracts.

The package keeps no private chat database. Authoritative state lives in the active World and is accessed through ordinary World Systems and Principal-filtered Projections.
