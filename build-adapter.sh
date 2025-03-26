# Useful for debugging:
# export CARGO_PROFILE_RELEASE_DEBUG=2
# export WIT_BINDGEN_DEBUG=1
export RUSTFLAGS="-Zoom=panic"

wasm-tools component wit --wasm wit -o lib/package.wasm

cargo build \
      -p virtual-adapter \
      --release \
      --target wasm32-unknown-unknown \
      -Z build-std=std,panic_abort \
      -Z build-std-features=panic_immediate_abort
cp target/wasm32-unknown-unknown/release/virtual_adapter.wasm lib/virtual_adapter.wasm

cargo build \
      -p virtual-adapter \
      --release \
      --target wasm32-unknown-unknown \
      --features debug \
      -Z build-std=std,panic_abort \
      -Z build-std-features=panic_immediate_abort
cp target/wasm32-unknown-unknown/release/virtual_adapter.wasm lib/virtual_adapter.debug.wasm
