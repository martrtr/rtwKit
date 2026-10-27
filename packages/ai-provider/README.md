# rtwKit AI Provider — first OpenAI-compatible adapter

This is an ordinary, replaceable Rintawa package that provides the versioned, provider-neutral service rintawa.inference.text@1. Core only routes service bytes and authenticated observer leases.

## Behavior

A World-local consumer invokes describe or generate. A valid generate request starts a bounded nonblocking Chat Completions POST and returns an opaque read-only async-operation observer token bound to the immediate authenticated service caller. The caller polls the generic observer for ordered provisional text-delta JSON bytes, then the typed completion metadata or a terminal failure. The provider cannot mutate World state; a Narrator must validate the completed result and submit a separate authoritative effect-fenced WorldCommand.

The adapter supports OpenAI-compatible streaming SSE, including arbitrary byte splits, UTF-8, choices[0].delta.content, provider model and finish reason, final optional token usage, backpressure, cancellation and strict output limits. Tool calls, multimodal inputs, the Narrator and provider-family parity are not implemented in this first slice. The ordinary Manage settings section now edits nonsecret connection preferences. Unsupported tool-call finishes fail closed.

## Connection and operator grants

By default the provider uses https://api.openai.com/v1 and model gpt-4o-mini. The component requests the runtime permissions http-request and background-task, and separately requests secret-read for ai.api_keys.openai. The Host must grant the exact HTTP origin and the runtime/secret permissions. Missing permissions or secrets return a redacted rejection without starting HTTP.

The nonsecret component-scoped preference connection supports JSON such as:

    {"base_url":"http://localhost:1234/v1","model":"local-model","requires_api_key":false}

The renderer-neutral Manage > AI Providers section edits a local, component-scoped connection draft and explicitly saves it through the generic preferences boundary. This never grants HTTP origins or secret-read rights, and never exposes API keys. Invalid/stale actions fail closed; settings do not mutate active requests. Loopback HTTP requires an explicit Host origin grant. The provider does not store API keys in preferences, package files, logs or World state. Host network address, TLS and redirect controls remain authoritative.

Requests run in cooperative Host task callbacks; at most four requests are in flight. Partial text remains provisional. Consumer backpressure pauses HTTP polling rather than silently dropping events, and a provider stop revokes active operations.

## Build and verification

Run cargo test -p rintawa-inference-contracts -p rintawa-ai-provider, then bash packages/ai-provider/build.sh. The build produces packages/ai-provider/build/rtw/runtime.wasm as a real WebAssembly Component. The tests cover bounded request validation, SSE splitting at every byte boundary, UTF-8 text, usage, malformed events and output limits.

The provider now includes a bounded Manage settings UI with explicit save/discard/defaults and revision-safe patches. After saving, `Check connection / list models` sends an explicitly requested read-only `GET /models` through the generic bounded Host HTTP boundary. The request never runs on each keystroke or at startup. Results are bounded, sorted, deduplicated and can be selected as a draft model; a separate Save is still mandatory. Unauthorized/rate-limited/timeout/invalid-response diagnostics are redacted, do not include HTTP error bodies or keys, and are not persisted as canonical World events. A discovered list is a hint, not a guarantee that a particular model will generate text. It is not yet a complete user-visible AI chat feature. It requires World-local configuration and a Narrator consumer for the durable Chat response.
