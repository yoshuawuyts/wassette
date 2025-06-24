# component2json

A Rust library for converting WebAssembly Components to JSON Schema and handling WebAssembly Interface Type (WIT) value conversions.

## Usage

```rust
use component2json::{component_exports_to_json_schema, json_to_vals, val_to_json};
use wasmtime::component::{Component, Type};
use wasmtime::Engine;

// Create a WebAssembly engine with component model enabled
let mut config = wasmtime::Config::new();
config.wasm_component_model(true);
let engine = Engine::new(&config)?;

// Load your component
let component = Component::from_file(&engine, "path/to/component.wasm")?;

// Get JSON schema for all exported functions
let schema = component_exports_to_json_schema(&component, &engine, true);

// To convert JSON to WIT Val arguments, you must provide the expected types.
// These would typically be derived from inspecting a function's parameters.
let func_param_types = vec![
    ("name".to_string(), Type::String),
    ("value".to_string(), Type::U32),
];

// Convert a JSON object to WIT values according to the function's parameter types
let json_args = serde_json::json!({
    "name": "example",
    "value": 42
});
let wit_vals = json_to_vals(&json_args, &func_param_types)?;

// Convert WIT values back to JSON
let json_result = vals_to_json(&wit_vals);
```

## Type Conversion Specification

### WIT to JSON Schema

#### Primitive Types

| WIT Type | JSON Schema |
|----------|-------------|
| `bool` | `{"type": "boolean"}` |
| `s8`, `s16`, `s32`, `s64` | `{"type": "number"}` |
| `u8`, `u16`, `u32`, `u64` | `{"type": "number"}` |
| `float32`, `float64` | `{"type": "number"}` |
| `char` | `{"type": "string", "description": "1 unicode codepoint"}` |
| `string` | `{"type": "string"}` |

#### Composite Types

##### Lists

```json
{
    "type": "array",
    "items": <schema-of-element-type>
}
```

##### Records

```json
{
    "type": "object",
    "properties": {
        "<field-name>": <schema-of-field-type>
    },
    "required": ["<field-names>"]
}
```

##### Tuples

```json
{
    "type": "array",
    "prefixItems": [<schema-of-each-type>],
    "minItems": <length>,
    "maxItems": <length>
}
```

##### Variants

```json
{
    "oneOf": [
        {
            "type": "object",
            "properties": {
                "tag": { "const": "<case-name>" },
                "val": <schema-of-payload-type>
            },
            "required": ["tag", "val"]
        },
        {
            "type": "object",
            "properties": {
                "tag": { "const": "<case-name>" }
            },
            "required": ["tag"]
        }
    ]
}
```

##### Enums

```json
{
    "type": "string",
    "enum": ["<enum-values>"]
}
```

##### Options

```json
{
    "anyOf": [
        { "type": "null" },
        <schema-of-inner-type>
    ]
}
```

##### Results

```json
{
    "oneOf": [
        {
            "type": "object",
            "properties": {
                "ok": <schema-of-ok-type>
            },
            "required": ["ok"]
        },
        {
            "type": "object",
            "properties": {
                "err": <schema-of-err-type>
            },
            "required": ["err"]
        }
    ]
}
```

##### Flags

```json
{
    "type": "array",
    "items": { "type": "string" }
}
```

##### Resources

```json
{
    "type": "string",
    "description": "<own'd|borrow'd> resource: <resource-name>"
}
```
