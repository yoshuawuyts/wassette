test:
    cargo test --workspace -- --nocapture
    cargo test --test integration_test -- --nocapture

build mode="debug":
    mkdir -p bin
    cargo build --workspace {{ if mode == "release" { "--release" } else { "" } }}
    cp target/{{ mode }}/mcp-wasmtime bin/
    
build-examples mode="debug":
    mkdir -p bin
    cargo build --target wasm32-wasip2 {{ if mode == "release" { "--release" } else { "" } }} --manifest-path examples/fetch-rs/Cargo.toml
    cargo build --target wasm32-wasip2 {{ if mode == "release" { "--release" } else { "" } }} --manifest-path examples/filesystem/Cargo.toml
    cd examples/get-weather && just build
    cp examples/fetch-rs/target/wasm32-wasip2/{{ mode }}/fetch_rs.wasm bin/fetch-rs.wasm
    cp examples/filesystem/target/wasm32-wasip2/{{ mode }}/filesystem.wasm bin/filesystem.wasm
    cp examples/get-weather/weather.wasm bin/get-weather.wasm

clean:
    cargo clean
    rm -rf bin

run RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --policy-file policy.yaml

run-filesystem RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --plugin-dir ./examples/filesystem2 --policy-file ./examples/filesystem2/policy.yaml

# Requires an openweather API key in the environment variable OPENWEATHER_API_KEY
run-get-weather RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --plugin-dir ./examples/get-weather --policy-file ./examples/get-weather/policy.yaml

run-fetch-rs RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin mcp-wasmtime serve --plugin-dir ./examples/fetch-rs --policy-file ./examples/fetch-rs/policy.yaml

