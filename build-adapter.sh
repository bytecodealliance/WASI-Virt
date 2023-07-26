cargo +nightly build -p virtual-adapter --target wasm32-wasi --release -Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort
cp target/wasm32-wasi/release/virtual_adapter.wasm lib/
