# Useful for debugging:
# export CARGO_PROFILE_RELEASE_DEBUG=2
# export WIT_BINDGEN_DEBUG=1
export RUSTFLAGS="-Zoom=panic"

VERSIONS="0_2_1 0_2_3"

for version in $VERSIONS
do
    echo -e "[info] building component WIT for [$version]...";
    wasm-tools component wit --wasm wit/$version -o lib/package-wasi$version.wasm

    echo -e "[info] building virtual adapters for version [$version]...";
    cargo build \
        -p virtual-adapter \
        --release \
        --target wasm32-unknown-unknown \
        --no-default-features \
        --features wasi-$version \
        -Z build-std=std,panic_abort \
        -Z build-std-features=panic_immediate_abort
    cp target/wasm32-unknown-unknown/release/virtual_adapter.wasm lib/virtual_adapter-wasi$version.wasm

    cargo build \
        -p virtual-adapter \
        --release \
        --target wasm32-unknown-unknown \
        --no-default-features \
        --features debug,wasi-$version \
        -Z build-std=std,panic_abort \
        -Z build-std-features=panic_immediate_abort
    cp target/wasm32-unknown-unknown/release/virtual_adapter.wasm lib/virtual_adapter-wasi$version.debug.wasm

done
