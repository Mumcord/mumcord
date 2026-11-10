//! The part of the Bovnar C API (`bovnar.h`, `bovnar_dom.h` of Bovnar 2.1.0) the SDK uses:
//! the event writer into memory and the DOM reader. The `bovnar` crate's own binding cannot
//! be used on `wasm32-unknown-unknown` because its build script compiles the C sources, so the
//! SDK declares the few functions it needs and links them from the prebuilt library there
//! (from the `bovnar` crate natively). The layouts mirror the C structs of that version
//! exactly; `scripts/build-bovnar-wasm.sh` pins the same version.

use std::ffi::{c_char, c_int, c_void};

/// `BVN_MAX_UNIT_COMPONENTS`.
const MAX_UNIT_COMPONENTS: usize = 32;

// bvnr_event_t
pub const EV_ASSIGNMENT_START: c_int = 1;
pub const EV_STRUCT_START: c_int = 4;
pub const EV_STRUCT_END: c_int = 5;
pub const EV_ARRAY_ROW_START: c_int = 6;
pub const EV_ARRAY_ROW_END: c_int = 7;
pub const EV_DATA: c_int = 9;

// error_code_t
pub const ERROR_SINK_BUFFER_EXHAUSTED: c_int = 31;

// token_type_t
pub const TOKEN_IDENTIFIER: c_int = 0;
pub const TOKEN_STRING: c_int = 1;
pub const TOKEN_NUMBER: c_int = 2;
pub const TOKEN_ARRAY_NUMBER: c_int = 5;
pub const TOKEN_ARRAY_STRING: c_int = 6;
pub const TOKEN_NULL_VALUE: c_int = 9;
pub const TOKEN_BOOL: c_int = 15;

// value_type_family_t
pub const VT_SINT: c_int = 2;
pub const VT_UINT: c_int = 3;

// bvn_dom_type_t
pub const DOM_NULL: c_int = 0;
pub const DOM_INT: c_int = 1;
pub const DOM_FLOAT: c_int = 2;
pub const DOM_STRING: c_int = 3;
pub const DOM_STRUCT: c_int = 6;
pub const DOM_ARRAY: c_int = 7;
pub const DOM_BOOL: c_int = 9;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ValueTypeSpec {
    pub family: c_int,
    pub width: u32,
    pub base: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ValueUnitPrefix {
    system: c_int,
    id: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ValueUnitComponent {
    base: c_int,
    exponent: c_int,
    prefix: ValueUnitPrefix,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ValueUnit {
    num_components: u32,
    components: [ValueUnitComponent; MAX_UNIT_COMPONENTS],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Converted {
    unit: ValueUnit,
    text: *const c_char,
    length: u32,
    base: u32,
    num: *mut c_void,
    den: *mut c_void,
}

/// `bvnr_data_t`: the payload of one writer event.
#[repr(C)]
pub struct Data {
    pub token_type: c_int,
    pub value_type: ValueTypeSpec,
    value_unit: ValueUnit,
    pub data: *const c_void,
    pub length: u32,
    frac_data: *const c_void,
    frac_length: u32,
    unit_source: *const c_void,
    unit_source_length: u32,
    converted: bool,
    conv: Converted,
}

impl Data {
    pub fn zeroed() -> Self {
        // SAFETY: every field is an integer, a bool, a nullable pointer or an array of those,
        // for which all-zero is valid (and is what the C API documents as "unset").
        unsafe { std::mem::zeroed() }
    }
}

/// `bvnr_write_flags_t`.
#[repr(C)]
pub struct WriteFlags {
    max_identifier_length: u16,
    max_string_length: u16,
    max_number_length: u16,
    max_symbol_length: u16,
    max_reference_length: u16,
    max_array_items: u64,
    max_text_bytes: u64,
    max_file_size: u64,
    pub max_struct_nesting: u8,
    pub max_array_nesting: u8,
    userdata: *mut c_void,
    on_event: Option<unsafe extern "C" fn()>,
    continue_on_error: bool,
    on_error: Option<unsafe extern "C" fn()>,
    unit_flags: u32,
    emit_version: bool,
    reserved: [u64; 4],
}

impl WriteFlags {
    pub fn zeroed() -> Self {
        // SAFETY: as for `Data`; zero selects the library defaults.
        unsafe { std::mem::zeroed() }
    }
}

/// `bvn_dom_entry_t`.
#[repr(C)]
pub struct DomEntry {
    pub key: *const c_char,
    pub value: *mut c_void,
}

unsafe extern "C" {
    pub fn bvnr_writer_create() -> *mut c_void;
    pub fn bvnr_writer_destroy(w: *mut c_void);
    pub fn bvnr_open_write_mem(
        w: *mut c_void,
        buf: *mut c_void,
        cap: u64,
        pretty: bool,
        options: *mut WriteFlags,
    ) -> bool;
    pub fn bvnr_write_event(w: *mut c_void, ev: c_int, data: *mut Data) -> bool;
    pub fn bvnr_write_finish(w: *mut c_void) -> bool;
    pub fn bvnr_writer_bytes_written(w: *mut c_void) -> u64;
    pub fn bvnr_writer_get_error(w: *const c_void) -> c_int;

    pub fn bvn_dom_parse(data: *mut c_void, len: u32) -> *mut c_void;
    pub fn bvn_dom_doc_destroy(doc: *mut c_void);
    pub fn bvn_dom_doc_get_parse_error(doc: *mut c_void) -> c_int;
    pub fn bvn_dom_doc_entries(doc: *mut c_void) -> *mut DomEntry;
    pub fn bvn_dom_doc_count(doc: *mut c_void) -> u32;
    pub fn bvn_dom_node_type(node: *mut c_void) -> c_int;
    pub fn bvn_dom_struct_entries(node: *mut c_void) -> *mut DomEntry;
    pub fn bvn_dom_struct_count(node: *mut c_void) -> u32;
    pub fn bvn_dom_array_count(node: *mut c_void) -> u32;
    pub fn bvn_dom_array_dims(node: *mut c_void) -> u32;
    pub fn bvn_dom_array_at(node: *mut c_void, index: u32) -> *mut c_void;
    pub fn bvn_dom_get_value_type(node: *mut c_void) -> ValueTypeSpec;
    pub fn bvn_dom_get_bool(node: *mut c_void, out: *mut bool) -> bool;
    pub fn bvn_dom_get_float(node: *mut c_void, out: *mut f64) -> bool;
    pub fn bvn_dom_get_i64(node: *mut c_void, out: *mut i64) -> bool;
    pub fn bvn_dom_get_u64(node: *mut c_void, out: *mut u64) -> bool;
    pub fn bvn_dom_get_string(node: *mut c_void, out: *mut *const c_void, len: *mut u32) -> bool;
}

// The C sizes and offsets of Bovnar 2.1.0 (unchanged since 2.0.0; measured with clang on both targets), so a layout
// mistake fails the build instead of corrupting memory.
#[cfg(target_pointer_width = "32")]
const _: () = assert!(
    size_of::<Data>() == 1096
        && std::mem::offset_of!(Data, data) == 532
        && size_of::<WriteFlags>() == 104
        && size_of::<DomEntry>() == 8
);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(
    size_of::<Data>() == 1136
        && std::mem::offset_of!(Data, data) == 536
        && size_of::<WriteFlags>() == 120
        && size_of::<DomEntry>() == 16
);
const _: () = assert!(
    std::mem::offset_of!(WriteFlags, max_struct_nesting) == 40 && size_of::<ValueTypeSpec>() == 12
);
