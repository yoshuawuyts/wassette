test:
    cargo test --workspace -- --nocapture
    cargo test --test integration_test -- --nocapture

build:
    cargo build --workspace

run RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --policy-file policy.yaml

run-filesystem RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --plugin-dir ./examples/filesystem2 --policy-file ./examples/filesystem2/policy.yaml

# Requires an openweather API key in the environment variable OPENWEATHER_API_KEY
run-get-weather RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --plugin-dir ./examples/get-weather --policy-file ./examples/get-weather/policy.yaml

run-fetch-rs RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --plugin-dir ./examples/fetch-rs --policy-file ./examples/fetch-rs/policy.yaml

