# rtwKit Chat (MVP)

Chat is a World-scoped extension, not a Core subsystem. Conversations and
messages live in the authoritative four-table World SQLite database.

Current commands:
- chat.create-conversation@1: creates a conversation object with title stored in
  chat.conversation@1 property.
- chat.send-message@1: creates a message object with chat.message@1 property and
  a relation-item from its conversation to the message.

Sending requires a bounded read from the same pinned World snapshot, and an
absent conversation is rejected. All writes use the authoritative single-writer
World Runtime and one atomic SQLite transaction. There are no separate Chat
tables, projections, event logs, polling tasks or Core UI DTOs. The WASM
component provides ordinary World System service contracts (v3).

This slice deliberately excludes the browser Chat UI, branching, Narrator and
AI. Those remain separate extension work. Historic Chat RTWs depend on removed
Projection/Portable UI APIs and cannot be activated by the new Core.

## Build and verify

With a matching PR #7 SDK and the wasm32-unknown-unknown Rust target:

    RUSTUP_TOOLCHAIN=stable CARGO_BUILD_JOBS=2 cargo test -p rintawa-chat --offline
    RUSTUP_TOOLCHAIN=stable CARGO_BUILD_JOBS=2 bash packages/chat/build.sh
    RUSTUP_TOOLCHAIN=stable CARGO_BUILD_JOBS=2 cargo test -p rintawa-chat --test chat_host_rtw_test --offline -- --ignored --nocapture

The ignored Host integration test needs the built RTW. It packs the actual
WASM component, installs it in Host CAS, activates it for a World, submits
commands through the component, restarts Host, and verifies persisted state.
