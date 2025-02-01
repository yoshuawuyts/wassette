use serde_json::{json, Map, Value};
use thiserror::Error;
use wasmtime::component::types::{ComponentFunc, ComponentItem};
use wasmtime::component::{Component, Type, Val};
use wasmtime::Engine;

#[derive(Error, Debug)]
pub enum ValError {
    /// The JSON number could not be interpreted as either an integer or a float.
    #[error("cannot interpret number as i64 or f64: {0}")]
    NumberError(String),

    /// A character field was invalid, for example an empty or multi-character string
    /// when you expected a single char.
    #[error("invalid char: {0}")]
    InvalidChar(String),

    /// An object had an unexpected shape for a particular conceptual type.
    #[error("expected object shape for {0}, found: {1}")]
    ShapeError(&'static str, String),

    /// A JSON object was recognized, but does not match any known variant shape.
    #[error("unknown object shape: {0:?}")]
    UnknownShape(serde_json::Map<String, Value>),

    /// Could not interpret a resource from the JSON field(s).
    #[error("cannot interpret resource from JSON")]
    ResourceError,
}

fn type_to_json_schema(t: &Type) -> Value {
    match t {
        Type::Bool => json!({ "type": "boolean" }),
        Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::Float32
        | Type::Float64 => json!({ "type": "number" }),
        Type::Char => json!({
            "type": "string",
            "description": "1 unicode codepoint"
        }),
        Type::String => json!({ "type": "string" }),

        // represent a `list<T>` as an array with items = schema-of-T
        Type::List(list_handle) => {
            let elem_schema = type_to_json_schema(&list_handle.ty());
            json!({
                "type": "array",
                "items": elem_schema
            })
        }

        Type::Record(r) => {
            let mut props = serde_json::Map::new();
            let mut required_fields = Vec::new();
            for field in r.fields() {
                required_fields.push(field.name.to_string());
                props.insert(field.name.to_string(), type_to_json_schema(&field.ty));
            }
            json!({
                "type": "object",
                "properties": props,
                "required": required_fields
            })
        }

        Type::Tuple(tup) => {
            let items: Vec<Value> = tup.types().map(|ty| type_to_json_schema(&ty)).collect();
            json!({
                "type": "array",
                "prefixItems": items,
                "minItems": items.len(),
                "maxItems": items.len()
            })
        }

        Type::Variant(variant_handle) => {
            let mut cases_schema = Vec::new();
            for case in variant_handle.cases() {
                let case_name = case.name;
                if let Some(ref payload_ty) = case.ty {
                    cases_schema.push(json!({
                        "type": "object",
                        "properties": {
                            "tag": { "const": case_name },
                            "val": type_to_json_schema(payload_ty)
                        },
                        "required": ["tag", "val"]
                    }));
                } else {
                    cases_schema.push(json!({
                        "type": "object",
                        "properties": {
                            "tag": { "const": case_name },
                        },
                        "required": ["tag"]
                    }));
                }
            }
            json!({ "oneOf": cases_schema })
        }

        Type::Enum(enum_handle) => {
            let names: Vec<&str> = enum_handle.names().collect();
            json!({
                "type": "string",
                "enum": names
            })
        }

        Type::Option(opt_handle) => {
            let inner_schema = type_to_json_schema(&opt_handle.ty());
            json!({
                "anyOf": [
                    { "type": "null" },
                    inner_schema
                ]
            })
        }

        Type::Result(res_handle) => {
            let ok_schema = res_handle
                .ok()
                .map(|ok_ty| type_to_json_schema(&ok_ty))
                .unwrap_or(json!({ "type": "null" }));

            let err_schema = res_handle
                .err()
                .map(|err_ty| type_to_json_schema(&err_ty))
                .unwrap_or(json!({ "type": "null" }));

            json!({
                "oneOf": [
                    {
                      "type": "object",
                      "properties": {
                        "ok": ok_schema
                      },
                      "required": ["ok"]
                    },
                    {
                      "type": "object",
                      "properties": {
                        "err": err_schema
                      },
                      "required": ["err"]
                    }
                ]
            })
        }

        Type::Flags(flags_handle) => {
            let mut props = serde_json::Map::new();
            for name in flags_handle.names() {
                props.insert(name.to_string(), json!({"type":"boolean"}));
            }
            json!({
                "type": "object",
                "properties": props
            })
        }

        Type::Own(r) => {
            json!({
                "type": "string",
                "description": format!("own'd resource: {:?}", r)
            })
        }
        Type::Borrow(r) => {
            json!({
                "type": "string",
                "description": format!("borrow'd resource: {:?}", r)
            })
        }
    }
}

fn component_func_to_schema(name: &str, func: &ComponentFunc, output: bool) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for (param_name, param_type) in func.params() {
        required.push(param_name.to_string());
        properties.insert(param_name.to_string(), type_to_json_schema(&param_type));
    }

    let input_schema = json!({
        "type": "object",
        "properties": properties,
        "required": required
    });

    let mut tool_obj = serde_json::Map::new();
    tool_obj.insert("name".to_string(), json!(name));
    tool_obj.insert(
        "description".to_string(),
        json!(format!("Auto-generated schema for function '{name}'")),
    );
    tool_obj.insert("inputSchema".to_string(), input_schema);

    if output {
        let mut results_iter = func.results();
        let output_schema = match results_iter.len() {
            0 => None,
            1 => Some(type_to_json_schema(&results_iter.next().unwrap())),
            _ => {
                let schemas: Vec<_> = results_iter.map(|ty| type_to_json_schema(&ty)).collect();
                Some(json!({
                    "type": "array",
                    "items": schemas
                }))
            }
        };
        if let Some(o) = output_schema {
            tool_obj.insert("outputSchema".to_string(), o);
        }
    }
    json!(tool_obj)
}

fn gather_exported_functions(
    export_name: &str,
    previous_name: Option<String>,
    item: &ComponentItem,
    engine: &Engine,
    results: &mut Vec<Value>,
    output: bool,
) {
    match item {
        ComponentItem::ComponentFunc(func) => {
            let name = if let Some(prefix) = previous_name {
                format!("{}.{}", prefix, export_name)
            } else {
                export_name.to_string()
            };
            results.push(component_func_to_schema(&name, func, output));
        }
        ComponentItem::Component(sub_component) => {
            let previous_name = Some(export_name.to_string());
            for (export_name, export_item) in sub_component.exports(engine) {
                gather_exported_functions(
                    &export_name,
                    previous_name.clone(),
                    &export_item,
                    engine,
                    results,
                    output,
                );
            }
        }
        ComponentItem::ComponentInstance(instance) => {
            let previous_name = Some(export_name.to_string());
            for (export_name, export_item) in instance.exports(engine) {
                gather_exported_functions(
                    &export_name,
                    previous_name.clone(),
                    &export_item,
                    engine,
                    results,
                    output,
                );
            }
        }
        ComponentItem::CoreFunc(_)
        | ComponentItem::Module(_)
        | ComponentItem::Type(_)
        | ComponentItem::Resource(_) => {}
    }
}

fn object_to_val(obj: &Map<String, Value>) -> Result<Val, ValError> {
    let mut fields = Vec::new();
    for (k, v) in obj {
        fields.push((k.clone(), json_to_val(v)?));
    }
    Ok(Val::Record(fields))
}

pub fn component_exports_to_json_schema(
    component: &Component,
    engine: &Engine,
    output: bool,
) -> Value {
    let mut tools_array = Vec::new();

    for (export_name, export_item) in component.component_type().exports(engine) {
        gather_exported_functions(
            export_name,
            None,
            &export_item,
            engine,
            &mut tools_array,
            output,
        );
    }

    json!({ "tools": tools_array })
}

/// Parses a single `serde_json::Value` into one `Val`.
pub fn json_to_val(value: &Value) -> Result<Val, ValError> {
    match value {
        Value::Null => Ok(Val::Option(None)),
        Value::Bool(b) => Ok(Val::Bool(*b)),
        Value::Number(num) => {
            if let Some(i) = num.as_i64() {
                Ok(Val::S64(i))
            } else if let Some(f) = num.as_f64() {
                Ok(Val::Float64(f))
            } else {
                Err(ValError::NumberError(format!("{num:?}")))
            }
        }
        Value::String(s) => Ok(Val::String(s.clone())),
        Value::Array(arr) => {
            let mut vals = Vec::new();
            for item in arr {
                vals.push(json_to_val(item)?);
            }
            Ok(Val::List(vals))
        }
        Value::Object(obj) => object_to_val(obj),
    }
}

pub fn json_to_vals(value: &Value) -> Result<Vec<Val>, ValError> {
    match value {
        Value::Object(obj) => {
            let mut results = Vec::new();
            for (_, v) in obj {
                let subval = json_to_val(v)?;
                results.push(subval);
            }
            Ok(results)
        }
        _ => {
            let single = json_to_val(value)?;
            Ok(vec![single])
        }
    }
}

pub fn vals_to_json(vals: &[Val]) -> Value {
    match vals.len() {
        0 => Value::Null,
        1 => val_to_json(&vals[0]),
        _ => {
            let mut map = Map::new();
            for (i, v) in vals.iter().enumerate() {
                map.insert(format!("val{i}"), val_to_json(v));
            }
            Value::Object(map)
        }
    }
}

fn val_to_json(val: &Val) -> Value {
    match val {
        Val::Bool(b) => Value::Bool(*b),
        Val::S8(n) => Value::Number((*n as i64).into()),
        Val::U8(n) => Value::Number((*n as u64).into()),
        Val::S16(n) => Value::Number((*n as i64).into()),
        Val::U16(n) => Value::Number((*n as u64).into()),
        Val::S32(n) => Value::Number((*n as i64).into()),
        Val::U32(n) => Value::Number((*n as u64).into()),
        Val::S64(n) => Value::Number((*n).into()),
        Val::U64(n) => Value::Number((*n).into()),
        Val::Float32(f) => serde_json::Number::from_f64(*f as f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(f.to_string())),
        Val::Float64(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(f.to_string())),
        Val::Char(c) => Value::String(c.to_string()),
        Val::String(s) => Value::String(s.clone()),

        Val::List(list) => Value::Array(list.iter().map(val_to_json).collect()),
        Val::Record(fields) => {
            let mut map = Map::new();
            for (k, v) in fields {
                map.insert(k.clone(), val_to_json(v));
            }
            Value::Object(map)
        }
        Val::Tuple(items) => Value::Array(items.iter().map(val_to_json).collect()),

        Val::Variant(tag, payload) => {
            let mut obj = Map::new();
            obj.insert("tag".to_string(), Value::String(tag.clone()));
            if let Some(val_box) = payload {
                obj.insert("val".to_string(), val_to_json(val_box));
            }
            Value::Object(obj)
        }
        Val::Enum(s) => Value::String(s.clone()),

        Val::Option(None) => Value::Null,
        Val::Option(Some(val_box)) => val_to_json(val_box),

        Val::Result(Ok(opt_box)) => {
            let mut obj = Map::new();
            obj.insert(
                "ok".to_string(),
                match opt_box {
                    Some(v) => val_to_json(v),
                    None => Value::Null,
                },
            );
            Value::Object(obj)
        }
        Val::Result(Err(opt_box)) => {
            let mut obj = Map::new();
            obj.insert(
                "err".to_string(),
                match opt_box {
                    Some(v) => val_to_json(v),
                    None => Value::Null,
                },
            );
            Value::Object(obj)
        }

        Val::Flags(flags) => Value::Array(flags.iter().map(|f| Value::String(f.clone())).collect()),
        Val::Resource(res) => Value::String(format!("resource: {:?}", res)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wasmtime::component::Val;

    #[test]
    fn test_string_val_conversion() {
        let json_val = json!("hello");
        let val = json_to_val(&json_val).unwrap();
        assert!(matches!(val, Val::String(ref s) if s == "hello"));
        assert_eq!(val_to_json(&val), json_val);
    }

    #[test]
    fn test_list_val_conversion() {
        let json_val = json!([1, 2, 3]);
        let val = json_to_val(&json_val).unwrap();
        assert!(matches!(val, Val::List(_)));
        assert_eq!(val_to_json(&val), json_val);
    }

    #[test]
    fn test_result_val_conversion() {
        let ok_json = json!({"ok": "success"});
        let ok_val = Val::Result(Ok(Some(Box::new(Val::String("success".to_string())))));
        assert_eq!(val_to_json(&ok_val), ok_json);

        let err_json = json!({"err": "error"});
        let err_val = Val::Result(Err(Some(Box::new(Val::String("error".to_string())))));
        assert_eq!(val_to_json(&err_val), err_json);
    }

    #[test]
    fn test_tuple_val_conversion() {
        let json_val = json!(["hello", 42]);
        let val = Val::Tuple(vec![Val::String("hello".to_string()), Val::S64(42)]);
        assert_eq!(val_to_json(&val), json_val);
    }

    #[test]
    fn test_generate_function_schema() {
        let engine = Engine::default();
        let wat = r#"(component
            (type (component
                (type (component
                    (type (list u8))
                    (type (tuple string 0))
                    (type (list 1))
                    (type (result 2 (error string)))
                    (type (func (param "name" string) (param "wit" 0) (result 3)))
                    (export "generate" (func (type 4)))
                ))
                (export "foo:foo/foo" (component (type 0)))
            ))
            (export "foo" (type 0))
            (@custom "package-docs" "\00{}")
            (@producers (processed-by "wit-component" "0.223.0"))
        )"#;
        let component = Component::new(&engine, wat).unwrap();
        let schema = component_exports_to_json_schema(&component, &engine, true);

        let tools = schema.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);

        let generate_tool = &tools[0];
        assert_eq!(generate_tool.get("name").unwrap(), "foo:foo/foo.generate");

        let input_schema = generate_tool.get("inputSchema").unwrap();
        let properties = input_schema.get("properties").unwrap().as_object().unwrap();
        assert!(properties.contains_key("name"));
        assert!(properties.contains_key("wit"));

        let output_schema = generate_tool.get("outputSchema").unwrap();
        assert!(output_schema.get("oneOf").is_some());
    }
}
