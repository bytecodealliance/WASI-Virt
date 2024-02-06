wasm-tools component wit --wasm wit -o lib/package.wasm

cargo +nightly build -p virtual-adapter --target wasm32-wasi --release -Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort &&
    cp target/wasm32-wasi/release/virtual_adapter.wasm lib/virtual_adapter.wasm

cargo +nightly build -p virtual-adapter --target wasm32-wasi --release --features debug -Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort &&
    cp target/wasm32-wasi/release/virtual_adapter.wasm lib/virtual_adapter.debug.wasm
