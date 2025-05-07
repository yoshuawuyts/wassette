test:
    cargo test --workspace -- --nocapture
    cargo test --test integration_test -- --nocapture

build:
    cargo build --workspace

run RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime 