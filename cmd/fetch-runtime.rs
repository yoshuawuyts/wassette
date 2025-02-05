use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

struct MyWasi {
    ctx: WasiCtx,
    table: wasmtime_wasi::ResourceTable,
    http: wasmtime_wasi_http::WasiHttpCtx,
}

impl WasiView for MyWasi {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
}

impl WasiHttpView for MyWasi {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Engine::new(&config)?;

    let component = Component::from_file(
        &engine,
        "/Users/mossaka/Developer/mossaka/mcp-wasmtime/examples/fetch-rs/fetch_rs.wasm",
    )?;

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)?;

    let mut store = Store::new(
        &engine,
        MyWasi {
            ctx: WasiCtx::builder()
                .inherit_args()
                .inherit_env()
                .inherit_stdio()
                .inherit_network()
                .allow_tcp(true)
                .allow_udp(true)
                .allow_ip_name_lookup(true)
                .preopened_dir("/", "/", DirPerms::READ, FilePerms::READ)?
                .build(),
            table: wasmtime_wasi::ResourceTable::default(),
            http: WasiHttpCtx::new(),
        },
    );
    let instance = linker.instantiate_async(&mut store, &component).await?;
    let fetch = instance
        .get_func(&mut store, "fetch")
        .expect("fetch function not found");

    let argument_vals = vec![Val::String("https://postman-echo.com/get".to_string())];
    let mut results = vec![Val::String("".to_string())];

    println!("Calling fetch function");
    fetch
        .call_async(&mut store, &argument_vals, &mut results)
        .await?;

    match &results[0] {
        Val::Result(Ok(ok)) => {
            if let Some(s) = ok.as_ref() {
                println!("Result: {}", format!("{:?}", s));
            }
        }
        Val::Result(Err(err)) => {
            if let Some(s) = err.as_ref() {
                println!("Error: {:?}", s);
            }
        }
        _ => println!("Unexpected result type: {:?}", results[0]),
    }
    Ok(())
}
