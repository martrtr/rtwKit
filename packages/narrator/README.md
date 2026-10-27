# rtwKit Narrator — World-scoped first reaction slice

An ordinary Rintawa extension consuming Chat's **principal-filtered World Projection** and the versioned neutral `rintawa.inference.text@1` service. The package has **no authority to mutate World state directly**: it may request an ordinary Host-authenticated `rintawa.chat.send-message@1` command. The Host/Chat System validate the exact Principal, entity Actor, ControlGrant and World consistency.

## Current implemented route

1. Activate only in an exact World scope. Subscribe to the **versioned** Chat `rintawa.chat.message-added@1` topic. Host delivers the full durable event envelope; Narrator checks world, schema, committed position and Chat message identity. No untrusted callback text becomes authority.
2. Request Chat's ordinary deferred, owner-scoped Projection from the Host. Check the selected conversation, leaf, immutable revision, author, parent and alternative match that event. Only a principal-owned, currently controllable human author is eligible. The live adapter still requires **exactly one** currently controlled linked-entity NPC participant; ambiguity or missing control fails closed. The library also exposes prepare_turn_for_responder for a future world-local policy to explicitly select a Host-authorized participant in group chat without treating other NPCs as this model's own Assistant history.
3. Build a bounded model-neutral prompt from the **selected branch ancestry**, not every message or raw World data. Retain chronological role history, omit earlier turns on strict byte budget, reject missing/cyclic branches and text-only-incompatible last messages. The only trusted System instruction is constant package code; imported Tavern text, participant display names, and Markdown remain lower-trust Chat data.
4. List exact World-local inference providers through normal Multiple service routing; select automatically **only when exactly one** eligible provider exists. Multiple candidates require explicit future policy/configuration; there is no implicit baseline fallback. Start a provider-owned operation with a **caller-bound read-only observer**.
5. Stream provisional typed text in bounded memory, reject unsupported/truncated completions, timeout, cancellation and error, close the observer on termination. **Do not save any partial model response**.
6. After the final provider completion, request a *fresh* Chat projection, revalidate the exact original selected leaf + immutable head revision + same controlled NPC actor, and submit the Chat-owned command with an explicit **expected World commit position**. Host System authority and stale-write checks remain decisive. One-shot reply controller prevents double submission within an attempt.

## Strict limits and failure handling

Input history is limited to 72 branch nodes, 32 visible messages and 32 KiB of text; the streamed output to 32 KiB. There is a bounded nonblocking operation poll budget, a separate projection revalidation budget and at most one in-memory generation per active extension instance. The selected branch is capped at 72 nodes while the indexed Chat projection is separately capped at 4,096 visible messages. Event envelopes are capped at 32 KiB, a bounded FIFO queue holds up to 128 pending triggers, and the active instance remembers up to 256 (MessageId, RevisionId) pairs to avoid repetitive requests. On overflow, the newest event is rejected with a bounded warning rather than replacing earlier accepted work. This still does not guarantee recovery after a restart. No provider bodies, API keys or raw Chat prompts are logged. Foreign World events, stale selections, missing permissions, ambiguous provider/actor configurations and cancelled or incomplete requests fail closed.

## Not yet claimed

**This is the first in-memory reaction adapter, not the finished MVP Narrator.** Event subscriptions, the bounded FIFO queue and provider operations are ephemeral. It does **not** implement crash/restart catch-up, durable EffectJob ↔ operation observer handoff, authoritative idempotent effect-fenced replies, a persistent per-World controller identity/ControlGrant setup, an NPC/character persona editor, lore/context-source composition, provider selection UI, alternate branch regeneration, or a full installed multi-package offline/online restart E2E. It responds only to already authorized linked-entity NPCs and direct-principal human messages, not automatic arbitrary NPC impersonation. A World without that exact control arrangement will correctly **not** produce an AI reply.

Core remains semantically neutral. The complete MVP requires generic host-authenticated durable operation observation and effect submission, World-local controller policy, and a real installed World/Chat/Provider integration test. No working tree change implies those requirements are complete.

## Development

From the rtwKit workspace run:

```bash
cargo +stable test -p rintawa-narrator -p rintawa-narrator-runtime
cargo +stable clippy -p rintawa-narrator -p rintawa-narrator-runtime --all-targets -- -D warnings
RUSTUP_TOOLCHAIN=stable bash packages/narrator/build.sh
```

The last command produces `build/rtw/runtime.wasm` for exact RTW local validation. Core and other concrete packages are untouched.
