test:
    cargo test --workspace -- --nocapture

build mode="debug":
    mkdir -p bin
    cargo build --workspace {{ if mode == "release" { "--release" } else { "" } }}
    cp target/{{ mode }}/weld-mcp-server bin/
    
build-examples mode="debug":
    mkdir -p bin
    cargo build --target wasm32-wasip2 {{ if mode == "release" { "--release" } else { "" } }} --manifest-path examples/fetch-rs/Cargo.toml
    cargo build --target wasm32-wasip2 {{ if mode == "release" { "--release" } else { "" } }} --manifest-path examples/filesystem/Cargo.toml
    (cd examples/get-weather && just build)
    (cd examples/time-server-js && just build)
    cp examples/fetch-rs/target/wasm32-wasip2/{{ mode }}/fetch_rs.wasm bin/fetch-rs.wasm
    cp examples/filesystem/target/wasm32-wasip2/{{ mode }}/filesystem.wasm bin/filesystem.wasm
    cp examples/get-weather/weather.wasm bin/get-weather.wasm
    cp examples/time-server-js/time.wasm bin/time-server-js.wasm
    
clean:
    cargo clean
    rm -rf bin

component2json path="examples/fetch-rs/target/wasm32-wasip2/release/fetch_rs.wasm":
    cargo run --bin component2json -p component2json -- {{ path }}

run RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin weld-mcp-server serve --policy-file policy.yaml

run-filesystem RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin weld-mcp-server serve --plugin-dir ./examples/filesystem --policy-file ./examples/filesystem/policy.yaml

# Requires an openweather API key in the environment variable OPENWEATHER_API_KEY
run-get-weather RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin weld-mcp-server serve --plugin-dir ./examples/get-weather --policy-file ./examples/get-weather/policy.yaml

run-fetch-rs RUST_LOG='info':
    RUST_LOG={{RUST_LOG}} cargo run --bin weld-mcp-server serve --plugin-dir ./examples/fetch-rs --policy-file ./examples/fetch-rs/policy.yaml

