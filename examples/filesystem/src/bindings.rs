#[allow(dead_code)]
pub mod mossaka {
    #[allow(dead_code)]
    pub mod mcp {
        #[allow(dead_code, clippy::all)]
        pub mod types {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            /// see: https://github.com/WebAssembly/component-model/issues/335
            pub type Json = _rt::String;
            #[derive(Clone)]
            pub struct ListToolsRequest {
                pub cursor: Option<_rt::String>,
                pub meta: Option<Json>,
            }
            impl ::core::fmt::Debug for ListToolsRequest {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("ListToolsRequest")
                        .field("cursor", &self.cursor)
                        .field("meta", &self.meta)
                        .finish()
                }
            }
            #[derive(Clone)]
            pub struct ToolDefinition {
                pub name: _rt::String,
                pub description: Option<_rt::String>,
                pub input_schema: Json,
            }
            impl ::core::fmt::Debug for ToolDefinition {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("ToolDefinition")
                        .field("name", &self.name)
                        .field("description", &self.description)
                        .field("input-schema", &self.input_schema)
                        .finish()
                }
            }
            #[derive(Clone)]
            pub struct ListToolsResponse {
                pub tools: _rt::Vec<ToolDefinition>,
                pub next_cursor: Option<_rt::String>,
                pub meta: Option<Json>,
            }
            impl ::core::fmt::Debug for ListToolsResponse {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("ListToolsResponse")
                        .field("tools", &self.tools)
                        .field("next-cursor", &self.next_cursor)
                        .field("meta", &self.meta)
                        .finish()
                }
            }
            #[derive(Clone)]
            pub struct CallToolRequest {
                pub name: _rt::String,
                pub arguments: Option<Json>,
                pub meta: Option<Json>,
            }
            impl ::core::fmt::Debug for CallToolRequest {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("CallToolRequest")
                        .field("name", &self.name)
                        .field("arguments", &self.arguments)
                        .field("meta", &self.meta)
                        .finish()
                }
            }
            #[derive(Clone)]
            pub struct ToolResponseText {
                pub text: _rt::String,
            }
            impl ::core::fmt::Debug for ToolResponseText {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("ToolResponseText").field("text", &self.text).finish()
                }
            }
            #[derive(Clone)]
            pub enum ToolResponseContent {
                Text(ToolResponseText),
            }
            impl ::core::fmt::Debug for ToolResponseContent {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    match self {
                        ToolResponseContent::Text(e) => {
                            f.debug_tuple("ToolResponseContent::Text").field(e).finish()
                        }
                    }
                }
            }
            #[derive(Clone)]
            pub struct CallToolResponse {
                pub content: _rt::Vec<ToolResponseContent>,
                pub is_error: Option<bool>,
                pub meta: Option<Json>,
            }
            impl ::core::fmt::Debug for CallToolResponse {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("CallToolResponse")
                        .field("content", &self.content)
                        .field("is-error", &self.is_error)
                        .field("meta", &self.meta)
                        .finish()
                }
            }
        }
    }
}
#[allow(dead_code)]
pub mod exports {
    #[allow(dead_code)]
    pub mod mossaka {
        #[allow(dead_code)]
        pub mod mcp {
            #[allow(dead_code, clippy::all)]
            pub mod tool_server {
                #[used]
                #[doc(hidden)]
                static __FORCE_SECTION_REF: fn() = super::super::super::super::__link_custom_section_describing_imports;
                use super::super::super::super::_rt;
                pub type CallToolRequest = super::super::super::super::mossaka::mcp::types::CallToolRequest;
                pub type CallToolResponse = super::super::super::super::mossaka::mcp::types::CallToolResponse;
                pub type ListToolsRequest = super::super::super::super::mossaka::mcp::types::ListToolsRequest;
                pub type ListToolsResponse = super::super::super::super::mossaka::mcp::types::ListToolsResponse;
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_call_tool_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                    arg2: i32,
                    arg3: *mut u8,
                    arg4: usize,
                    arg5: i32,
                    arg6: *mut u8,
                    arg7: usize,
                ) -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let bytes0 = _rt::Vec::from_raw_parts(arg0.cast(), len0, len0);
                    let result3 = T::call_tool(super::super::super::super::mossaka::mcp::types::CallToolRequest {
                        name: _rt::string_lift(bytes0),
                        arguments: match arg2 {
                            0 => None,
                            1 => {
                                let e = {
                                    let len1 = arg4;
                                    let bytes1 = _rt::Vec::from_raw_parts(
                                        arg3.cast(),
                                        len1,
                                        len1,
                                    );
                                    _rt::string_lift(bytes1)
                                };
                                Some(e)
                            }
                            _ => _rt::invalid_enum_discriminant(),
                        },
                        meta: match arg5 {
                            0 => None,
                            1 => {
                                let e = {
                                    let len2 = arg7;
                                    let bytes2 = _rt::Vec::from_raw_parts(
                                        arg6.cast(),
                                        len2,
                                        len2,
                                    );
                                    _rt::string_lift(bytes2)
                                };
                                Some(e)
                            }
                            _ => _rt::invalid_enum_discriminant(),
                        },
                    });
                    let ptr4 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    let super::super::super::super::mossaka::mcp::types::CallToolResponse {
                        content: content5,
                        is_error: is_error5,
                        meta: meta5,
                    } = result3;
                    let vec9 = content5;
                    let len9 = vec9.len();
                    let layout9 = _rt::alloc::Layout::from_size_align_unchecked(
                        vec9.len() * 12,
                        4,
                    );
                    let result9 = if layout9.size() != 0 {
                        let ptr = _rt::alloc::alloc(layout9).cast::<u8>();
                        if ptr.is_null() {
                            _rt::alloc::handle_alloc_error(layout9);
                        }
                        ptr
                    } else {
                        { ::core::ptr::null_mut() }
                    };
                    for (i, e) in vec9.into_iter().enumerate() {
                        let base = result9.add(i * 12);
                        {
                            use super::super::super::super::mossaka::mcp::types::ToolResponseContent as V8;
                            match e {
                                V8::Text(e) => {
                                    *base.add(0).cast::<u8>() = (0i32) as u8;
                                    let super::super::super::super::mossaka::mcp::types::ToolResponseText {
                                        text: text6,
                                    } = e;
                                    let vec7 = (text6.into_bytes()).into_boxed_slice();
                                    let ptr7 = vec7.as_ptr().cast::<u8>();
                                    let len7 = vec7.len();
                                    ::core::mem::forget(vec7);
                                    *base.add(8).cast::<usize>() = len7;
                                    *base.add(4).cast::<*mut u8>() = ptr7.cast_mut();
                                }
                            }
                        }
                    }
                    *ptr4.add(4).cast::<usize>() = len9;
                    *ptr4.add(0).cast::<*mut u8>() = result9;
                    match is_error5 {
                        Some(e) => {
                            *ptr4.add(8).cast::<u8>() = (1i32) as u8;
                            *ptr4.add(9).cast::<u8>() = (match e {
                                true => 1,
                                false => 0,
                            }) as u8;
                        }
                        None => {
                            *ptr4.add(8).cast::<u8>() = (0i32) as u8;
                        }
                    };
                    match meta5 {
                        Some(e) => {
                            *ptr4.add(12).cast::<u8>() = (1i32) as u8;
                            let vec10 = (e.into_bytes()).into_boxed_slice();
                            let ptr10 = vec10.as_ptr().cast::<u8>();
                            let len10 = vec10.len();
                            ::core::mem::forget(vec10);
                            *ptr4.add(20).cast::<usize>() = len10;
                            *ptr4.add(16).cast::<*mut u8>() = ptr10.cast_mut();
                        }
                        None => {
                            *ptr4.add(12).cast::<u8>() = (0i32) as u8;
                        }
                    };
                    ptr4
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_call_tool<T: Guest>(arg0: *mut u8) {
                    let l0 = *arg0.add(0).cast::<*mut u8>();
                    let l1 = *arg0.add(4).cast::<usize>();
                    let base5 = l0;
                    let len5 = l1;
                    for i in 0..len5 {
                        let base = base5.add(i * 12);
                        {
                            let l2 = i32::from(*base.add(0).cast::<u8>());
                            match l2 {
                                _ => {
                                    let l3 = *base.add(4).cast::<*mut u8>();
                                    let l4 = *base.add(8).cast::<usize>();
                                    _rt::cabi_dealloc(l3, l4, 1);
                                }
                            }
                        }
                    }
                    _rt::cabi_dealloc(base5, len5 * 12, 4);
                    let l6 = i32::from(*arg0.add(12).cast::<u8>());
                    match l6 {
                        0 => {}
                        _ => {
                            let l7 = *arg0.add(16).cast::<*mut u8>();
                            let l8 = *arg0.add(20).cast::<usize>();
                            _rt::cabi_dealloc(l7, l8, 1);
                        }
                    }
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_list_tools_cabi<T: Guest>(
                    arg0: i32,
                    arg1: *mut u8,
                    arg2: usize,
                    arg3: i32,
                    arg4: *mut u8,
                    arg5: usize,
                ) -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let result2 = T::list_tools(super::super::super::super::mossaka::mcp::types::ListToolsRequest {
                        cursor: match arg0 {
                            0 => None,
                            1 => {
                                let e = {
                                    let len0 = arg2;
                                    let bytes0 = _rt::Vec::from_raw_parts(
                                        arg1.cast(),
                                        len0,
                                        len0,
                                    );
                                    _rt::string_lift(bytes0)
                                };
                                Some(e)
                            }
                            _ => _rt::invalid_enum_discriminant(),
                        },
                        meta: match arg3 {
                            0 => None,
                            1 => {
                                let e = {
                                    let len1 = arg5;
                                    let bytes1 = _rt::Vec::from_raw_parts(
                                        arg4.cast(),
                                        len1,
                                        len1,
                                    );
                                    _rt::string_lift(bytes1)
                                };
                                Some(e)
                            }
                            _ => _rt::invalid_enum_discriminant(),
                        },
                    });
                    let ptr3 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    let super::super::super::super::mossaka::mcp::types::ListToolsResponse {
                        tools: tools4,
                        next_cursor: next_cursor4,
                        meta: meta4,
                    } = result2;
                    let vec9 = tools4;
                    let len9 = vec9.len();
                    let layout9 = _rt::alloc::Layout::from_size_align_unchecked(
                        vec9.len() * 28,
                        4,
                    );
                    let result9 = if layout9.size() != 0 {
                        let ptr = _rt::alloc::alloc(layout9).cast::<u8>();
                        if ptr.is_null() {
                            _rt::alloc::handle_alloc_error(layout9);
                        }
                        ptr
                    } else {
                        { ::core::ptr::null_mut() }
                    };
                    for (i, e) in vec9.into_iter().enumerate() {
                        let base = result9.add(i * 28);
                        {
                            let super::super::super::super::mossaka::mcp::types::ToolDefinition {
                                name: name5,
                                description: description5,
                                input_schema: input_schema5,
                            } = e;
                            let vec6 = (name5.into_bytes()).into_boxed_slice();
                            let ptr6 = vec6.as_ptr().cast::<u8>();
                            let len6 = vec6.len();
                            ::core::mem::forget(vec6);
                            *base.add(4).cast::<usize>() = len6;
                            *base.add(0).cast::<*mut u8>() = ptr6.cast_mut();
                            match description5 {
                                Some(e) => {
                                    *base.add(8).cast::<u8>() = (1i32) as u8;
                                    let vec7 = (e.into_bytes()).into_boxed_slice();
                                    let ptr7 = vec7.as_ptr().cast::<u8>();
                                    let len7 = vec7.len();
                                    ::core::mem::forget(vec7);
                                    *base.add(16).cast::<usize>() = len7;
                                    *base.add(12).cast::<*mut u8>() = ptr7.cast_mut();
                                }
                                None => {
                                    *base.add(8).cast::<u8>() = (0i32) as u8;
                                }
                            };
                            let vec8 = (input_schema5.into_bytes()).into_boxed_slice();
                            let ptr8 = vec8.as_ptr().cast::<u8>();
                            let len8 = vec8.len();
                            ::core::mem::forget(vec8);
                            *base.add(24).cast::<usize>() = len8;
                            *base.add(20).cast::<*mut u8>() = ptr8.cast_mut();
                        }
                    }
                    *ptr3.add(4).cast::<usize>() = len9;
                    *ptr3.add(0).cast::<*mut u8>() = result9;
                    match next_cursor4 {
                        Some(e) => {
                            *ptr3.add(8).cast::<u8>() = (1i32) as u8;
                            let vec10 = (e.into_bytes()).into_boxed_slice();
                            let ptr10 = vec10.as_ptr().cast::<u8>();
                            let len10 = vec10.len();
                            ::core::mem::forget(vec10);
                            *ptr3.add(16).cast::<usize>() = len10;
                            *ptr3.add(12).cast::<*mut u8>() = ptr10.cast_mut();
                        }
                        None => {
                            *ptr3.add(8).cast::<u8>() = (0i32) as u8;
                        }
                    };
                    match meta4 {
                        Some(e) => {
                            *ptr3.add(20).cast::<u8>() = (1i32) as u8;
                            let vec11 = (e.into_bytes()).into_boxed_slice();
                            let ptr11 = vec11.as_ptr().cast::<u8>();
                            let len11 = vec11.len();
                            ::core::mem::forget(vec11);
                            *ptr3.add(28).cast::<usize>() = len11;
                            *ptr3.add(24).cast::<*mut u8>() = ptr11.cast_mut();
                        }
                        None => {
                            *ptr3.add(20).cast::<u8>() = (0i32) as u8;
                        }
                    };
                    ptr3
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_list_tools<T: Guest>(arg0: *mut u8) {
                    let l0 = *arg0.add(0).cast::<*mut u8>();
                    let l1 = *arg0.add(4).cast::<usize>();
                    let base9 = l0;
                    let len9 = l1;
                    for i in 0..len9 {
                        let base = base9.add(i * 28);
                        {
                            let l2 = *base.add(0).cast::<*mut u8>();
                            let l3 = *base.add(4).cast::<usize>();
                            _rt::cabi_dealloc(l2, l3, 1);
                            let l4 = i32::from(*base.add(8).cast::<u8>());
                            match l4 {
                                0 => {}
                                _ => {
                                    let l5 = *base.add(12).cast::<*mut u8>();
                                    let l6 = *base.add(16).cast::<usize>();
                                    _rt::cabi_dealloc(l5, l6, 1);
                                }
                            }
                            let l7 = *base.add(20).cast::<*mut u8>();
                            let l8 = *base.add(24).cast::<usize>();
                            _rt::cabi_dealloc(l7, l8, 1);
                        }
                    }
                    _rt::cabi_dealloc(base9, len9 * 28, 4);
                    let l10 = i32::from(*arg0.add(8).cast::<u8>());
                    match l10 {
                        0 => {}
                        _ => {
                            let l11 = *arg0.add(12).cast::<*mut u8>();
                            let l12 = *arg0.add(16).cast::<usize>();
                            _rt::cabi_dealloc(l11, l12, 1);
                        }
                    }
                    let l13 = i32::from(*arg0.add(20).cast::<u8>());
                    match l13 {
                        0 => {}
                        _ => {
                            let l14 = *arg0.add(24).cast::<*mut u8>();
                            let l15 = *arg0.add(28).cast::<usize>();
                            _rt::cabi_dealloc(l14, l15, 1);
                        }
                    }
                }
                pub trait Guest {
                    fn call_tool(req: CallToolRequest) -> CallToolResponse;
                    fn list_tools(req: ListToolsRequest) -> ListToolsResponse;
                }
                #[doc(hidden)]
                macro_rules! __export_mossaka_mcp_tool_server_0_1_0_cabi {
                    ($ty:ident with_types_in $($path_to_types:tt)*) => {
                        const _ : () = { #[export_name =
                        "mossaka:mcp/tool-server@0.1.0#call-tool"] unsafe extern "C" fn
                        export_call_tool(arg0 : * mut u8, arg1 : usize, arg2 : i32, arg3
                        : * mut u8, arg4 : usize, arg5 : i32, arg6 : * mut u8, arg7 :
                        usize,) -> * mut u8 { $($path_to_types)*::
                        _export_call_tool_cabi::<$ty > (arg0, arg1, arg2, arg3, arg4,
                        arg5, arg6, arg7) } #[export_name =
                        "cabi_post_mossaka:mcp/tool-server@0.1.0#call-tool"] unsafe
                        extern "C" fn _post_return_call_tool(arg0 : * mut u8,) {
                        $($path_to_types)*:: __post_return_call_tool::<$ty > (arg0) }
                        #[export_name = "mossaka:mcp/tool-server@0.1.0#list-tools"]
                        unsafe extern "C" fn export_list_tools(arg0 : i32, arg1 : * mut
                        u8, arg2 : usize, arg3 : i32, arg4 : * mut u8, arg5 : usize,) ->
                        * mut u8 { $($path_to_types)*:: _export_list_tools_cabi::<$ty >
                        (arg0, arg1, arg2, arg3, arg4, arg5) } #[export_name =
                        "cabi_post_mossaka:mcp/tool-server@0.1.0#list-tools"] unsafe
                        extern "C" fn _post_return_list_tools(arg0 : * mut u8,) {
                        $($path_to_types)*:: __post_return_list_tools::<$ty > (arg0) } };
                    };
                }
                #[doc(hidden)]
                pub(crate) use __export_mossaka_mcp_tool_server_0_1_0_cabi;
                #[repr(align(4))]
                struct _RetArea([::core::mem::MaybeUninit<u8>; 32]);
                static mut _RET_AREA: _RetArea = _RetArea(
                    [::core::mem::MaybeUninit::uninit(); 32],
                );
            }
        }
    }
}
mod _rt {
    pub use alloc_crate::string::String;
    pub use alloc_crate::vec::Vec;
    #[cfg(target_arch = "wasm32")]
    pub fn run_ctors_once() {
        wit_bindgen_rt::run_ctors_once();
    }
    pub unsafe fn string_lift(bytes: Vec<u8>) -> String {
        if cfg!(debug_assertions) {
            String::from_utf8(bytes).unwrap()
        } else {
            String::from_utf8_unchecked(bytes)
        }
    }
    pub unsafe fn invalid_enum_discriminant<T>() -> T {
        if cfg!(debug_assertions) {
            panic!("invalid enum discriminant")
        } else {
            core::hint::unreachable_unchecked()
        }
    }
    pub use alloc_crate::alloc;
    pub unsafe fn cabi_dealloc(ptr: *mut u8, size: usize, align: usize) {
        if size == 0 {
            return;
        }
        let layout = alloc::Layout::from_size_align_unchecked(size, align);
        alloc::dealloc(ptr, layout);
    }
    extern crate alloc as alloc_crate;
}
/// Generates `#[no_mangle]` functions to export the specified type as the
/// root implementation of all generated traits.
///
/// For more information see the documentation of `wit_bindgen::generate!`.
///
/// ```rust
/// # macro_rules! export{ ($($t:tt)*) => (); }
/// # trait Guest {}
/// struct MyType;
///
/// impl Guest for MyType {
///     // ...
/// }
///
/// export!(MyType);
/// ```
#[allow(unused_macros)]
#[doc(hidden)]
macro_rules! __export_mcp_impl {
    ($ty:ident) => {
        self::export!($ty with_types_in self);
    };
    ($ty:ident with_types_in $($path_to_types_root:tt)*) => {
        $($path_to_types_root)*::
        exports::mossaka::mcp::tool_server::__export_mossaka_mcp_tool_server_0_1_0_cabi!($ty
        with_types_in $($path_to_types_root)*:: exports::mossaka::mcp::tool_server);
    };
}
#[doc(inline)]
pub(crate) use __export_mcp_impl as export;
#[cfg(target_arch = "wasm32")]
#[link_section = "component-type:wit-bindgen:0.30.0:mcp:encoded world"]
#[doc(hidden)]
pub static __WIT_BINDGEN_COMPONENT_TYPE: [u8; 830] = *b"\
\0asm\x0d\0\x01\0\0\x19\x16wit-component-encoding\x04\0\x07\xc4\x05\x01A\x02\x01\
A\x08\x01B\x15\x01s\x04\0\x04json\x03\0\0\x01ks\x01k\x01\x01r\x02\x06cursor\x02\x04\
meta\x03\x04\0\x12list-tools-request\x03\0\x04\x01r\x03\x04names\x0bdescription\x02\
\x0cinput-schema\x01\x04\0\x0ftool-definition\x03\0\x06\x01p\x07\x01r\x03\x05too\
ls\x08\x0bnext-cursor\x02\x04meta\x03\x04\0\x13list-tools-response\x03\0\x09\x01\
r\x03\x04names\x09arguments\x03\x04meta\x03\x04\0\x11call-tool-request\x03\0\x0b\
\x01r\x01\x04texts\x04\0\x12tool-response-text\x03\0\x0d\x01q\x01\x04text\x01\x0e\
\0\x04\0\x15tool-response-content\x03\0\x0f\x01p\x10\x01k\x7f\x01r\x03\x07conten\
t\x11\x08is-error\x12\x04meta\x03\x04\0\x12call-tool-response\x03\0\x13\x03\x01\x17\
mossaka:mcp/types@0.1.0\x05\0\x02\x03\0\0\x11call-tool-request\x02\x03\0\0\x12ca\
ll-tool-response\x02\x03\0\0\x12list-tools-request\x02\x03\0\0\x13list-tools-res\
ponse\x01B\x0c\x02\x03\x02\x01\x01\x04\0\x11call-tool-request\x03\0\0\x02\x03\x02\
\x01\x02\x04\0\x12call-tool-response\x03\0\x02\x02\x03\x02\x01\x03\x04\0\x12list\
-tools-request\x03\0\x04\x02\x03\x02\x01\x04\x04\0\x13list-tools-response\x03\0\x06\
\x01@\x01\x03req\x01\0\x03\x04\0\x09call-tool\x01\x08\x01@\x01\x03req\x05\0\x07\x04\
\0\x0alist-tools\x01\x09\x04\x01\x1dmossaka:mcp/tool-server@0.1.0\x05\x05\x04\x01\
\x15mossaka:mcp/mcp@0.1.0\x04\0\x0b\x09\x01\0\x03mcp\x03\0\0\0G\x09producers\x01\
\x0cprocessed-by\x02\x0dwit-component\x070.215.0\x10wit-bindgen-rust\x060.30.0";
#[inline(never)]
#[doc(hidden)]
pub fn __link_custom_section_describing_imports() {
    wit_bindgen_rt::maybe_link_cabi_realloc();
}
