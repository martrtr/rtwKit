#!/usr/bin/env bash
set -euo pipefail

package_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$package_dir/../.." && pwd)"
output="$package_dir/build/rtw"
target_dir="$repo_root/target/wasm32-unknown-unknown/release"
handler_core="$target_dir/rintawa_character_library_handler.wasm"
runtime_core="$target_dir/rintawa_character_library_runtime.wasm"

if [[ -z "${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER:-}" ]]; then
    for candidate in wasm-ld rust-lld ld.lld; do
        if linker="$(command -v "$candidate" 2>/dev/null)"; then
            export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER="$linker"
            break
        fi
    done
fi

if [[ -z "${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER:-}" && -d /nix/store ]]; then
    linker="$(find /nix/store -maxdepth 3 -type f -path '*/bin/wasm-ld' -print -quit 2>/dev/null || true)"
    if [[ -n "$linker" ]]; then
        export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER="$linker"
    fi
fi

cargo build --locked --release --target wasm32-unknown-unknown \
    --manifest-path "$package_dir/content-handler/Cargo.toml"
cargo build --locked --release --target wasm32-unknown-unknown \
    --manifest-path "$package_dir/runtime/Cargo.toml"
rm -rf "$output"
mkdir -p "$output"
cp -R "$package_dir/rtw/." "$output/"
cargo run --quiet --locked -p rtwkit-componentize -- "$handler_core" "$output/content-handler.wasm"
cargo run --quiet --locked -p rtwkit-componentize -- "$runtime_core" "$output/runtime.wasm"
