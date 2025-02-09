# component2json

A Rust library for converting WebAssembly Components to JSON Schema and handling WebAssembly Interface Type (WIT) value conversions.

## Overview

`component2json` provides three main functionalities:
1. Converting WebAssembly component exports to JSON Schema
2. Converting JSON values to WIT values
3. Converting WIT values to JSON values

## Usage

```rust
use component2json::{component_exports_to_json_schema, json_to_vals, vals_to_json};
use wasmtime::component::Component;
use wasmtime::Engine;

// Create a WebAssembly engine with component model enabled
let mut config = wasmtime::Config::new();
config.wasm_component_model(true);
let engine = Engine::new(&config)?;

// Load your component
let component = Component::from_file(&engine, "path/to/component.wasm")?;

// Get JSON schema for all exported functions
let schema = component_exports_to_json_schema(&component, &engine, true);

// Convert JSON arguments to WIT values
let json_args = serde_json::json!({
    "name": "example",
    "value": 42
});
let wit_vals = json_to_vals(&json_args)?;

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

## Error Handling

The library provides a `ValError` enum for handling conversion errors:

- `NumberError`: When a JSON number cannot be interpreted as either an integer or float
- `InvalidChar`: When a character field is invalid (empty or multi-character string)
- `ShapeError`: When an object has an unexpected shape for a particular type
- `UnknownShape`: When a JSON object doesn't match any known variant shape
- `ResourceError`: When a resource cannot be interpreted from JSON

## Examples

### Converting Component Exports to JSON Schema

```rust
let mut config = wasmtime::Config::new();
config.wasm_component_model(true);
let engine = Engine::new(&config)?;

// Load a component with filesystem operations
let component = Component::from_file(&engine, "filesystem.wasm")?;
let schema = component_exports_to_json_schema(&component, &engine, true);

// The schema will contain a "tools" array with function descriptions
// Each function will have:
// - name: fully qualified function name
// - description: auto-generated description
// - inputSchema: JSON schema for function parameters
// - outputSchema: JSON schema for function return value (if output=true)
```

### Converting Between JSON and WIT Values

```rust
// JSON to WIT conversion examples
let json_null = serde_json::json!(null);
assert!(matches!(json_to_val(&json_null)?, Val::Option(None)));

let json_bool = serde_json::json!(true);
assert!(matches!(json_to_val(&json_bool)?, Val::Bool(true)));

let json_number = serde_json::json!(42);
assert!(matches!(json_to_val(&json_number)?, Val::S64(42)));

let json_string = serde_json::json!("hello");
assert!(matches!(json_to_val(&json_string)?, Val::String(s) if s == "hello"));

let json_array = serde_json::json!([1, 2, 3]);
if let Val::List(list) = json_to_val(&json_array)? {
    assert_eq!(list.len(), 3);
}

let json_object = serde_json::json!({"key": "value"});
if let Val::Record(fields) = json_to_val(&json_object)? {
    assert_eq!(fields[0].0, "key");
}

// WIT to JSON conversion examples
let wit_bool = Val::Bool(false);
assert_eq!(val_to_json(&wit_bool), serde_json::json!(false));

let wit_string = Val::String("test".to_string());
assert_eq!(val_to_json(&wit_string), serde_json::json!("test"));

let wit_list = Val::List(vec![Val::S64(1), Val::S64(2)]);
assert_eq!(val_to_json(&wit_list), serde_json::json!([1, 2]));
```
