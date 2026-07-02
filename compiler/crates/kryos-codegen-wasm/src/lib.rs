//! WebAssembly codegen backend for Kryos (v0.1 — minimum viable).
//!
//! This backend lowers a subset of Kryos MIR to a WebAssembly module that runs
//! in `wasmtime` (with WASI) and in browsers (with a small JS shim for `println`).
//!
//! ## Supported (v0.1)
//!
//! - Integer (`i64`) and float (`f64`) arithmetic
//! - Booleans, comparisons, logical ops, bitwise ops
//! - `if`/`else`, `while`, `for`, recursion, mutual recursion
//! - Function definitions and direct calls
//! - `println(i64)` and `println(f64)` via host-imported `kryos_print_i64` /
//!   `kryos_print_f64` functions
//! - Constant string literals via `kryos_print_str(offset, len)`
//! - `return`, basic block control flow
//!
//! ## Not supported yet (v0.1)
//!
//! - Heap-allocated values: strings beyond literals, arrays, maps, structs, enums
//! - ARC, closures, traits, vtables, channels, spawn, actors
//! - The full stdlib (HTTP, file I/O, regex, JSON, etc.)
//! - These will return a `BackendError` with a clear "unsupported in WASM v0.1"
//!   message during codegen, leaving the door open for future growth.
//!
//! ## Module structure
//!
//! The emitted WASM module:
//!
//! - Imports `kryos_print_i64`, `kryos_print_f64`, `kryos_print_str` from `env`
//! - Exports a `memory` (1 page minimum) for the host to read string literals from
//! - Exports every user function under its Kryos name
//! - Exports `main` as the canonical entry point if present
//!
//! Strings are laid out at fixed offsets in the data segment; the codegen
//! records `(offset, len)` for each `ConstString` and the host reads them out
//! of memory.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::collections::HashMap;

use kryos_mir::ir::{
    BasicBlock, BlockId, Constant, Instruction, MirBinOp, MirFunction, MirModule, MirType,
    MirUnOp, Operand, RValue, Terminator,
};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction as W, MemArg, MemorySection, MemoryType, Module,
    TypeSection, ValType,
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Errors raised during WASM codegen.
#[derive(Debug, Clone)]
pub struct WasmCodegenError {
    pub message: String,
}

impl WasmCodegenError {
    fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }

    fn unsupported(feature: &str) -> Self {
        Self {
            message: format!(
                "WASM backend v0.1 does not yet support: {feature}. \
                 Use --backend cranelift or --backend llvm for full feature coverage."
            ),
        }
    }
}

impl std::fmt::Display for WasmCodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WasmCodegenError {}

/// Options for WASM module emission.
///
/// Kryos wasm modules import their runtime from the `env` host module (the
/// JS-host contract -- see docs/wasm-stdlib.md and tools/wasm-host/run.mjs).
/// WASI is NOT supported: there is no wasi_snapshot_preview1 emission, and
/// wasmtime cannot run kryos modules. A real WASI target is a future
/// feature, not an option toggle.
#[derive(Debug, Clone, Default)]
pub struct WasmOptions {}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Emit a complete WASM module for the given MIR module.
pub fn emit_module(module: &MirModule, options: &WasmOptions) -> Result<Vec<u8>, WasmCodegenError> {
    let mut cg = WasmCodegen::new(options.clone());
    cg.emit(module)?;
    Ok(cg.finish())
}

// ---------------------------------------------------------------------------
// Backend trait implementation
// ---------------------------------------------------------------------------

/// Driver-compatible WASM backend.
pub struct WasmBackend {
    options: WasmOptions,
    _inc: RefCell<()>,
}

impl WasmBackend {
    pub fn new(options: WasmOptions) -> Self {
        Self {
            options,
            _inc: RefCell::new(()),
        }
    }
}

impl Default for WasmBackend {
    fn default() -> Self {
        Self::new(WasmOptions::default())
    }
}

impl kryos_driver::Backend for WasmBackend {
    fn compile(&self, module: &MirModule) -> Result<Vec<u8>, kryos_driver::BackendError> {
        emit_module(module, &self.options)
            .map_err(|e| kryos_driver::BackendError::new(format!("wasm codegen: {e}")))
    }

    fn emit_ir(&self, _module: &MirModule) -> Result<String, kryos_driver::BackendError> {
        Err(kryos_driver::BackendError::unsupported(
            "wasm backend does not emit a text IR (use wasm2wat on the output .wasm)",
        ))
    }

    fn name(&self) -> &str {
        "wasm"
    }
}

// ---------------------------------------------------------------------------
// Internal: type lowering
// ---------------------------------------------------------------------------

/// The WASM value type a Kryos MIR type lowers to.
fn lower_type(ty: &MirType) -> Result<ValType, WasmCodegenError> {
    Ok(match ty {
        MirType::I8 | MirType::I16 | MirType::I32 | MirType::U8 | MirType::U16 | MirType::U32 => {
            ValType::I32
        }
        MirType::I64 | MirType::U64 | MirType::Bool | MirType::Char => ValType::I64,
        MirType::F32 => ValType::F32,
        MirType::F64 => ValType::F64,
        // For v0.2 we represent strings as packed i64 pointers:
        //   low 32 bits = byte offset into linear memory
        //   high 32 bits = byte length
        // This lets a string survive in a single local without a side table
        // and is what `len(s)`, string concat, and `println(s)` all decode.
        // Same scheme is used for arrays of i64 (offset, count_in_elements).
        MirType::Str => ValType::I64,
        // WASM v0.3: arrays are represented as packed i64 pointers, same
        // encoding as strings: low 32 bits = offset, high 32 bits = length.
        MirType::Array(_, _) => ValType::I64,
        // Structs are represented as a packed i64 array handle (same scheme as
        // arrays): the host allocates N slots and returns an opaque handle.
        // Field access uses array_get(handle, field_index).
        MirType::Struct(_) => ValType::I64,
        // Enums are represented as a packed i64 array handle with the same slot
        // encoding as structs: slot 0 = tag (variant index), slots 1..n = payload
        // fields. This reuses all the struct infrastructure already in place.
        MirType::Enum(_) => ValType::I64,
        // Tuples are represented as i64 array handles (same as structs):
        // slot i holds element i of the tuple.
        MirType::Tuple(_) => ValType::I64,
        MirType::Void => {
            // Caller should special-case void; this is a placeholder.
            return Err(WasmCodegenError::new(
                "internal: lower_type called on Void; caller should special-case",
            ));
        }
        other => {
            return Err(WasmCodegenError::unsupported(&format!(
                "type `{other}` (WASM v0.3 supports i64/f64/bool/str + [T] arrays)"
            )));
        }
    })
}

/// Returns true if the type is `void` (no WASM value).
fn is_void(ty: &MirType) -> bool {
    matches!(ty, MirType::Void)
}

// ---------------------------------------------------------------------------
// Internal: the codegen state machine
// ---------------------------------------------------------------------------

struct WasmCodegen {
    options: WasmOptions,

    // WASM sections, built incrementally.
    types: TypeSection,
    imports: ImportSection,
    funcs: FunctionSection,
    memory: MemorySection,
    exports: ExportSection,
    code: CodeSection,
    data: DataSection,

    /// Number of distinct function signatures registered so far.
    type_count: u32,
    /// Total number of functions (imports + user functions) so far.
    /// WASM assigns function indices in import-then-defined order.
    func_count: u32,

    /// Map from Kryos function name -> WASM function index.
    fn_indices: HashMap<String, u32>,

    /// Map from Kryos function name -> signature index.
    fn_sigs: HashMap<String, u32>,

    /// Index of the imported `kryos_print_i64` function.
    print_i64_idx: u32,
    /// Index of the imported `kryos_print_f64` function.
    print_f64_idx: u32,
    /// Index of the imported `kryos_print_str` function (offset, len).
    print_str_idx: u32,
    /// Index of the imported `kryos_string_concat(off1,len1,off2,len2) -> i64`.
    /// Returns a packed (offset<<0)|(len<<32) pointer into linear memory.
    string_concat_idx: u32,
    /// Index of the imported `kryos_array_new(count) -> i64` (packed).
    array_new_idx: u32,
    /// Index of the imported `kryos_array_get(packed, index) -> i64`.
    array_get_idx: u32,
    /// Index of the imported `kryos_array_set(packed, index, value)`.
    array_set_idx: u32,
    // ---- WASM v0.4: browser/web host imports (DOM/canvas/fetch) ----
    /// `kryos_dom_set_text(id_off, id_len, txt_off, txt_len)` -> void.
    dom_set_text_idx: u32,
    /// `kryos_dom_get_value(id_off, id_len) -> packed string i64`.
    dom_get_value_idx: u32,
    /// `kryos_alert(off, len)` -> void.
    alert_idx: u32,
    /// `kryos_canvas_fill_rect(id_off, id_len, x, y, w, h, color_off, color_len)` -> void.
    canvas_fill_rect_idx: u32,
    /// `kryos_canvas_clear(id_off, id_len)` -> void.
    canvas_clear_idx: u32,
    /// `kryos_fetch_text(url_off, url_len) -> packed string i64` (sync xhr).
    fetch_text_idx: u32,

    // ---- WASM v2.3: stdlib parity primitives ----
    /// `kryos_string_length(packed) -> i32`.
    string_length_idx: u32,
    /// `kryos_string_slice(packed, start, end) -> packed`.
    string_slice_idx: u32,
    /// `kryos_string_to_upper(packed) -> packed`.
    string_to_upper_idx: u32,
    /// `kryos_string_to_lower(packed) -> packed`.
    string_to_lower_idx: u32,
    /// `kryos_string_trim(packed) -> packed`.
    string_trim_idx: u32,
    /// `kryos_string_index_of(packed_haystack, packed_needle) -> i32` (-1 if not found).
    string_index_of_idx: u32,
    /// `kryos_string_parse_int(packed) -> i64`.
    string_parse_int_idx: u32,
    /// `kryos_string_parse_float(packed) -> f64`.
    string_parse_float_idx: u32,
    /// `kryos_array_length(packed) -> i32`.
    array_length_idx: u32,
    /// `kryos_array_push(packed, value) -> packed` (returns new packed handle).
    array_push_idx: u32,
    /// `kryos_array_pop(packed) -> i64` (returns -1 sentinel if empty).
    array_pop_idx: u32,
    /// `kryos_json_parse(packed_str) -> handle_i64` (opaque doc handle).
    json_parse_idx: u32,
    /// `kryos_json_stringify(handle) -> packed_str`.
    json_stringify_idx: u32,
    /// `kryos_json_get_int(handle, key_off, key_len) -> i64`.
    json_get_int_idx: u32,
    /// `kryos_json_get_str(handle, key_off, key_len) -> packed_str`.
    json_get_str_idx: u32,
    /// `kryos_regex_test(packed_pat, packed_subject) -> i32` (0/1).
    regex_test_idx: u32,
    /// `kryos_regex_replace(packed_pat, packed_subject, packed_repl) -> packed`.
    regex_replace_idx: u32,
    /// `kryos_http_fetch(method_off,method_len, url_off,url_len, body_off,body_len) -> packed`.
    http_fetch_idx: u32,
    /// `kryos_to_string_i64(v: i64) -> packed_str` — converts i64 to its decimal string.
    to_string_i64_idx: u32,
    /// `kryos_to_string_f64(v: f64) -> packed_str` — converts f64 to its decimal string.
    to_string_f64_idx: u32,

    /// Struct definitions: maps struct name -> ordered list of (field_name, field_type).
    /// Used for field-index resolution when lowering RValue::Struct / RValue::Field.
    struct_defs: HashMap<String, Vec<(String, MirType)>>,

    /// String literal interning: maps the literal -> (offset, len) in memory.
    string_table: HashMap<String, (u32, u32)>,
    /// Next free byte in the data segment.
    string_cursor: u32,
    /// All string bytes concatenated, written to the data segment at finish.
    string_bytes: Vec<u8>,
}

impl WasmCodegen {
    fn new(options: WasmOptions) -> Self {
        Self {
            options,
            types: TypeSection::new(),
            imports: ImportSection::new(),
            funcs: FunctionSection::new(),
            memory: MemorySection::new(),
            exports: ExportSection::new(),
            code: CodeSection::new(),
            data: DataSection::new(),
            type_count: 0,
            func_count: 0,
            fn_indices: HashMap::new(),
            fn_sigs: HashMap::new(),
            print_i64_idx: 0,
            print_f64_idx: 0,
            print_str_idx: 0,
            string_concat_idx: 0,
            array_new_idx: 0,
            array_get_idx: 0,
            array_set_idx: 0,
            dom_set_text_idx: 0,
            dom_get_value_idx: 0,
            alert_idx: 0,
            canvas_fill_rect_idx: 0,
            canvas_clear_idx: 0,
            fetch_text_idx: 0,
            string_length_idx: 0,
            string_slice_idx: 0,
            string_to_upper_idx: 0,
            string_to_lower_idx: 0,
            string_trim_idx: 0,
            string_index_of_idx: 0,
            string_parse_int_idx: 0,
            string_parse_float_idx: 0,
            array_length_idx: 0,
            array_push_idx: 0,
            array_pop_idx: 0,
            json_parse_idx: 0,
            json_stringify_idx: 0,
            json_get_int_idx: 0,
            json_get_str_idx: 0,
            regex_test_idx: 0,
            regex_replace_idx: 0,
            http_fetch_idx: 0,
            to_string_i64_idx: 0,
            to_string_f64_idx: 0,
            struct_defs: HashMap::new(),
            string_table: HashMap::new(),
            // Reserve the first 16 bytes so offset 0 stays sentinel-free.
            string_cursor: 16,
            string_bytes: vec![0u8; 16],
        }
    }

    /// Top-level emit.
    fn emit(&mut self, module: &MirModule) -> Result<(), WasmCodegenError> {
        // 0. Cache struct definitions for field-index lookups during codegen.
        self.struct_defs = module.struct_defs.clone();

        // 1. Register host imports for printing.
        self.register_host_imports();

        // 2. Plan: assign signature indices and function indices for every
        //    user function up-front so calls can resolve by name regardless
        //    of definition order.
        self.plan_functions(module)?;

        // 3. Emit memory section: 1 page (64KB) for now. Enough for string
        //    literals; the user code itself doesn't allocate yet.
        self.memory.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });

        // 4. Export memory so the host can read string literals.
        self.exports.export("memory", ExportKind::Memory, 0);

        // 5. Walk every user function and emit its body.
        for func in &module.functions {
            self.emit_function(func)?;
        }

        // 6. Write the accumulated string-literal bytes into a data segment
        //    starting at offset 0.
        if !self.string_bytes.is_empty() {
            self.data
                .active(0, &ConstExpr::i32_const(0), self.string_bytes.iter().copied());
        }

        // (`main` is already exported by `plan_functions`, no need to re-export.)

        Ok(())
    }

    /// Register the three host print imports. They get function indices
    /// 0, 1, 2 (imports come before user-defined functions in WASM).
    fn register_host_imports(&mut self) {
        let env_module = "env";

        // sig 0: (i64) -> ()
        self.types.ty().function(vec![ValType::I64], vec![]);
        self.imports
            .import(env_module, "kryos_print_i64", wasm_encoder::EntityType::Function(0));
        self.print_i64_idx = 0;
        self.func_count = 1;
        self.type_count = 1;

        // sig 1: (f64) -> ()
        self.types.ty().function(vec![ValType::F64], vec![]);
        self.imports
            .import(env_module, "kryos_print_f64", wasm_encoder::EntityType::Function(1));
        self.print_f64_idx = 1;
        self.func_count = 2;
        self.type_count = 2;

        // sig 2: (i32, i32) -> () — (offset, len) of a string in linear memory
        self.types
            .ty()
            .function(vec![ValType::I32, ValType::I32], vec![]);
        self.imports
            .import(env_module, "kryos_print_str", wasm_encoder::EntityType::Function(2));
        self.print_str_idx = 2;
        self.func_count = 3;
        self.type_count = 3;

        // sig 3: (i32, i32, i32, i32) -> i64  — string concat
        self.types.ty().function(
            vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            vec![ValType::I64],
        );
        self.imports.import(
            env_module,
            "kryos_string_concat",
            wasm_encoder::EntityType::Function(3),
        );
        self.string_concat_idx = 3;
        self.func_count = 4;
        self.type_count = 4;

        // sig 4: (i32) -> i64  — array_new(count) -> packed (offset,count)
        self.types
            .ty()
            .function(vec![ValType::I32], vec![ValType::I64]);
        self.imports.import(
            env_module,
            "kryos_array_new",
            wasm_encoder::EntityType::Function(4),
        );
        self.array_new_idx = 4;
        self.func_count = 5;
        self.type_count = 5;

        // sig 5: (i64, i32) -> i64  — array_get(packed, index)
        self.types
            .ty()
            .function(vec![ValType::I64, ValType::I32], vec![ValType::I64]);
        self.imports.import(
            env_module,
            "kryos_array_get",
            wasm_encoder::EntityType::Function(5),
        );
        self.array_get_idx = 5;
        self.func_count = 6;
        self.type_count = 6;

        // sig 6: (i64, i32, i64) -> ()  — array_set(packed, index, value)
        self.types
            .ty()
            .function(vec![ValType::I64, ValType::I32, ValType::I64], vec![]);
        self.imports.import(
            env_module,
            "kryos_array_set",
            wasm_encoder::EntityType::Function(6),
        );
        self.array_set_idx = 6;
        self.func_count = 7;
        self.type_count = 7;

        // ====================================================================
        // WASM v0.4: browser host imports
        // ====================================================================

        // sig 7: (i32,i32,i32,i32) -> ()  — dom_set_text(id_off,id_len,txt_off,txt_len)
        self.types.ty().function(
            vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            vec![],
        );
        self.imports.import(env_module, "kryos_dom_set_text",
            wasm_encoder::EntityType::Function(7));
        self.dom_set_text_idx = 7;
        self.func_count = 8;
        self.type_count = 8;

        // sig 8: (i32,i32) -> i64  — dom_get_value(id_off,id_len) -> packed str
        self.types.ty().function(
            vec![ValType::I32, ValType::I32],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_dom_get_value",
            wasm_encoder::EntityType::Function(8));
        self.dom_get_value_idx = 8;
        self.func_count = 9;
        self.type_count = 9;

        // sig 9: (i32,i32) -> ()  — alert(off,len)
        self.types.ty().function(
            vec![ValType::I32, ValType::I32],
            vec![],
        );
        self.imports.import(env_module, "kryos_alert",
            wasm_encoder::EntityType::Function(9));
        self.alert_idx = 9;
        self.func_count = 10;
        self.type_count = 10;

        // sig 10: (i32,i32,i64,i64,i64,i64,i32,i32) -> ()
        //   canvas_fill_rect(id_off,id_len, x,y,w,h, color_off,color_len)
        self.types.ty().function(
            vec![
                ValType::I32, ValType::I32,
                ValType::I64, ValType::I64, ValType::I64, ValType::I64,
                ValType::I32, ValType::I32,
            ],
            vec![],
        );
        self.imports.import(env_module, "kryos_canvas_fill_rect",
            wasm_encoder::EntityType::Function(10));
        self.canvas_fill_rect_idx = 10;
        self.func_count = 11;
        self.type_count = 11;

        // sig 11: (i32,i32) -> ()  — canvas_clear(id_off,id_len)
        self.types.ty().function(
            vec![ValType::I32, ValType::I32],
            vec![],
        );
        self.imports.import(env_module, "kryos_canvas_clear",
            wasm_encoder::EntityType::Function(11));
        self.canvas_clear_idx = 11;
        self.func_count = 12;
        self.type_count = 12;

        // sig 12: (i32,i32) -> i64  — fetch_text(url_off,url_len) -> packed str
        self.types.ty().function(
            vec![ValType::I32, ValType::I32],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_fetch_text",
            wasm_encoder::EntityType::Function(12));
        self.fetch_text_idx = 12;
        self.func_count = 13;
        self.type_count = 13;

        // ====================================================================
        // WASM v2.3: stdlib parity primitives
        //
        // Each block follows the pattern:
        //   * push a TypeSection signature
        //   * import the host function under env::kryos_<name>
        //   * record idx in self.<name>_idx
        //   * bump func_count + type_count
        //
        // All string-bearing primitives use the packed-i64 convention
        // (low 32 = byte offset, high 32 = byte length) consistent with
        // kryos_string_concat. Array primitives use the same scheme.
        // ====================================================================

        // string_length(packed) -> i32
        self.types.ty().function(vec![ValType::I64], vec![ValType::I32]);
        self.imports.import(env_module, "kryos_string_length",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_length_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // string_slice(packed, start, end) -> packed
        self.types.ty().function(
            vec![ValType::I64, ValType::I32, ValType::I32],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_string_slice",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_slice_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // string_to_upper(packed) -> packed
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_string_to_upper",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_to_upper_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // string_to_lower(packed) -> packed
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_string_to_lower",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_to_lower_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // string_trim(packed) -> packed
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_string_trim",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_trim_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // string_index_of(packed_haystack, packed_needle) -> i32 (-1 if absent)
        self.types.ty().function(
            vec![ValType::I64, ValType::I64],
            vec![ValType::I32],
        );
        self.imports.import(env_module, "kryos_string_index_of",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_index_of_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // string_parse_int(packed) -> i64
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_string_parse_int",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_parse_int_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // string_parse_float(packed) -> f64
        self.types.ty().function(vec![ValType::I64], vec![ValType::F64]);
        self.imports.import(env_module, "kryos_string_parse_float",
            wasm_encoder::EntityType::Function(self.type_count));
        self.string_parse_float_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // array_length(packed) -> i32
        self.types.ty().function(vec![ValType::I64], vec![ValType::I32]);
        self.imports.import(env_module, "kryos_array_length",
            wasm_encoder::EntityType::Function(self.type_count));
        self.array_length_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // array_push(packed, value) -> packed (new handle, since growth may reallocate)
        self.types.ty().function(
            vec![ValType::I64, ValType::I64],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_array_push",
            wasm_encoder::EntityType::Function(self.type_count));
        self.array_push_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // array_pop(packed) -> i64 (-1 sentinel when empty)
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_array_pop",
            wasm_encoder::EntityType::Function(self.type_count));
        self.array_pop_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // json_parse(packed_str) -> handle_i64
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_json_parse",
            wasm_encoder::EntityType::Function(self.type_count));
        self.json_parse_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // json_stringify(handle) -> packed_str
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_json_stringify",
            wasm_encoder::EntityType::Function(self.type_count));
        self.json_stringify_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // json_get_int(handle, key_off, key_len) -> i64
        self.types.ty().function(
            vec![ValType::I64, ValType::I32, ValType::I32],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_json_get_int",
            wasm_encoder::EntityType::Function(self.type_count));
        self.json_get_int_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // json_get_str(handle, key_off, key_len) -> packed_str
        self.types.ty().function(
            vec![ValType::I64, ValType::I32, ValType::I32],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_json_get_str",
            wasm_encoder::EntityType::Function(self.type_count));
        self.json_get_str_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // regex_test(packed_pat, packed_subject) -> i32 (0/1)
        self.types.ty().function(
            vec![ValType::I64, ValType::I64],
            vec![ValType::I32],
        );
        self.imports.import(env_module, "kryos_regex_test",
            wasm_encoder::EntityType::Function(self.type_count));
        self.regex_test_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // regex_replace(packed_pat, packed_subject, packed_repl) -> packed
        self.types.ty().function(
            vec![ValType::I64, ValType::I64, ValType::I64],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_regex_replace",
            wasm_encoder::EntityType::Function(self.type_count));
        self.regex_replace_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // http_fetch(method_off,len, url_off,len, body_off,len) -> packed_response
        self.types.ty().function(
            vec![
                ValType::I32, ValType::I32,
                ValType::I32, ValType::I32,
                ValType::I32, ValType::I32,
            ],
            vec![ValType::I64],
        );
        self.imports.import(env_module, "kryos_http_fetch",
            wasm_encoder::EntityType::Function(self.type_count));
        self.http_fetch_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // to_string_i64(v: i64) -> packed_str  — decimal formatting of an i64
        self.types.ty().function(vec![ValType::I64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_to_string_i64",
            wasm_encoder::EntityType::Function(self.type_count));
        self.to_string_i64_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;

        // to_string_f64(v: f64) -> packed_str  — decimal formatting of an f64
        self.types.ty().function(vec![ValType::F64], vec![ValType::I64]);
        self.imports.import(env_module, "kryos_to_string_f64",
            wasm_encoder::EntityType::Function(self.type_count));
        self.to_string_f64_idx = self.func_count;
        self.func_count += 1; self.type_count += 1;
    }

    /// Walk the module once to assign function indices and signature indices.
    fn plan_functions(&mut self, module: &MirModule) -> Result<(), WasmCodegenError> {
        for func in &module.functions {
            // Build the WASM function signature from the MIR parameter and
            // return types.
            let mut params = Vec::with_capacity(func.params.len());
            for p in &func.params {
                params.push(lower_type(&p.ty)?);
            }
            let results: Vec<ValType> = if is_void(&func.ret_ty) {
                vec![]
            } else {
                vec![lower_type(&func.ret_ty)?]
            };

            let sig_idx = self.type_count;
            self.types.ty().function(params, results);
            self.type_count += 1;

            self.funcs.function(sig_idx);
            let fn_idx = self.func_count;
            self.func_count += 1;

            self.fn_indices.insert(func.name.clone(), fn_idx);
            self.fn_sigs.insert(func.name.clone(), sig_idx);

            // Export every function so the host can call any of them (handy
            // for testing and library usage).
            self.exports
                .export(&func.name, ExportKind::Func, fn_idx);
        }
        Ok(())
    }

    /// Emit the body of one function.
    fn emit_function(&mut self, func: &MirFunction) -> Result<(), WasmCodegenError> {
        // Lower every local to a WASM val type so we can declare them up front.
        // Locals 0..params.len() are reserved for parameters by WASM convention.
        let n_params = func.params.len();
        let mut local_decls: Vec<(u32, ValType)> = Vec::new();

        // Group consecutive same-type locals to keep the function header small.
        let mut current_group: Option<(ValType, u32)> = None;
        for local in func.locals.iter().skip(n_params) {
            // Map every MIR local to *some* WASM val type so local indices
            // stay aligned. Void locals get i32 as a placeholder; we never
            // read or write them (see Assign / Drop handling).
            let vt = if is_void(&local.ty) {
                ValType::I32
            } else {
                match lower_type(&local.ty) {
                    Ok(t) => t,
                    Err(_) => {
                        // Unsupported local type — for v0.1 we lower it to i64
                        // as a placeholder. Any actual use will fail later,
                        // but unused locals stay harmless.
                        ValType::I64
                    }
                }
            };
            match current_group {
                Some((t, count)) if t == vt => current_group = Some((t, count + 1)),
                Some((t, count)) => {
                    local_decls.push((count, t));
                    current_group = Some((vt, 1));
                }
                None => current_group = Some((vt, 1)),
            }
        }
        if let Some((t, count)) = current_group {
            local_decls.push((count, t));
        }

        // v0.2: reserve ONE extra i64 scratch local at the very end of every
        // function. Index = func.locals.len(). Used by `emit_unpack_string`
        // to tee/dup a packed (offset|len) i64 without disturbing user
        // locals. Cost: 8 bytes per call frame — negligible.
        // Merge with the trailing group if it's already i64, else append.
        match local_decls.last_mut() {
            Some(entry) if entry.1 == ValType::I64 => entry.0 += 1,
            _ => local_decls.push((1, ValType::I64)),
        }

        let mut wfunc = Function::new(local_decls);

        // Lower the function body using a structured-control-flow translator
        // (`emit_block` walks the CFG and uses wasm's `block` / `loop` / `if`
        // to express the shapes we accept in v0.1).
        self.emit_function_body(func, &mut wfunc)?;

        // Every wasm function body must terminate with `end`. wasm-encoder
        // adds this for us when we call `code.function(&wfunc)`. But if the
        // last terminator we emitted was a Goto that fell through (no
        // Return / Unreachable instruction was emitted), we may need to push
        // a default return value for non-void functions.
        if !is_void(&func.ret_ty) {
            // Best-effort: push a zero of the right type so wasm validation
            // passes when the user code didn't end every path in a Return.
            match lower_type(&func.ret_ty) {
                Ok(ValType::I32) => {
                    wfunc.instruction(&W::I32Const(0));
                }
                Ok(ValType::I64) => {
                    wfunc.instruction(&W::I64Const(0));
                }
                Ok(ValType::F32) => {
                    wfunc.instruction(&W::F32Const(0.0));
                }
                Ok(ValType::F64) => {
                    wfunc.instruction(&W::F64Const(0.0));
                }
                _ => {}
            }
        }
        wfunc.instruction(&W::End);

        self.code.function(&wfunc);
        Ok(())
    }

    /// Emit a function body using a structured-control-flow strategy.
    ///
    /// We support the common shapes the Kryos front-end emits:
    ///
    /// 1. **Straight-line** — one or more blocks chained via `Goto`/`Return`.
    /// 2. **If/else (including chained elif)** — nested `Branch` arms that
    ///    eventually `Goto` a common join block.
    /// 3. **While loop** — header block ends in `Branch { cond, body, exit }`,
    ///    body ends in `Goto(header)`.
    ///
    /// More complex CFGs (irreducible loops, multi-entry blocks, exception
    /// handling) return an unsupported error.
    fn emit_function_body(
        &mut self,
        func: &MirFunction,
        wfunc: &mut Function,
    ) -> Result<(), WasmCodegenError> {
        let n_params = func.params.len();

        // Map `BlockId.0` (the *identifier*, not the array index) to the
        // index in `func.blocks`. MIR block ids are non-contiguous: the
        // front-end may skip numbers when blocks are pruned.
        let mut id_to_index: HashMap<u32, usize> = HashMap::new();
        for (i, b) in func.blocks.iter().enumerate() {
            id_to_index.insert(b.id.0, i);
        }

        let visited = vec![false; func.blocks.len()];
        let mut emitter = FnEmitter {
            cg: self,
            func,
            wfunc,
            n_params,
            id_to_index,
            visited,
        };
        // The entry block is always the first one in `func.blocks` (the
        // Cranelift backend relies on the same convention).
        let entry_id = func.blocks[0].id;
        emitter.emit_block(entry_id)?;
        Ok(())
    }

    /// Allocate (or return cached) string literal in the data segment.
    /// Returns (offset, length_in_bytes).
    fn intern_string(&mut self, s: &str) -> (u32, u32) {
        if let Some(&pair) = self.string_table.get(s) {
            return pair;
        }
        let bytes = s.as_bytes();
        let offset = self.string_cursor;
        let len = bytes.len() as u32;
        self.string_bytes.extend_from_slice(bytes);
        self.string_cursor += len;
        // Align to 4 for cleanliness.
        while self.string_cursor % 4 != 0 {
            self.string_bytes.push(0);
            self.string_cursor += 1;
        }
        self.string_table.insert(s.to_string(), (offset, len));
        (offset, len)
    }

    /// Finalize: stitch all sections together into the final module bytes.
    fn finish(self) -> Vec<u8> {
        let mut m = Module::new();
        m.section(&self.types);
        m.section(&self.imports);
        m.section(&self.funcs);
        m.section(&self.memory);
        m.section(&self.exports);
        m.section(&self.code);
        if !self.string_bytes.is_empty() {
            m.section(&self.data);
        }
        m.finish()
    }
}

// ---------------------------------------------------------------------------
// Per-function emitter — owns a mutable borrow of the codegen state plus the
// wasm-encoder `Function` builder.
// ---------------------------------------------------------------------------

struct FnEmitter<'a> {
    cg: &'a mut WasmCodegen,
    func: &'a MirFunction,
    wfunc: &'a mut Function,
    /// Number of formal parameters — retained for future use by passes
    /// that need to distinguish params from locals when emitting wasm
    /// `local.get` / `local.set` indices.
    #[allow(dead_code)]
    n_params: usize,
    /// Maps a `BlockId.0` to its index in `func.blocks`. Block ids are
    /// not contiguous, so we always go through this map.
    id_to_index: HashMap<u32, usize>,
    /// Indexed by *array index* in `func.blocks` (not by block id).
    visited: Vec<bool>,
}

impl<'a> FnEmitter<'a> {
    fn block_index(&self, id: BlockId) -> Result<usize, WasmCodegenError> {
        self.id_to_index.get(&id.0).copied().ok_or_else(|| {
            WasmCodegenError::new(format!("unknown block id bb{}", id.0))
        })
    }

    /// Retained for ergonomic block-id-to-block lookups by future passes
    /// that need the full `BasicBlock` rather than just the index.
    #[allow(dead_code)]
    fn block_by_id(&self, id: BlockId) -> Result<&'a BasicBlock, WasmCodegenError> {
        let idx = self.block_index(id)?;
        Ok(&self.func.blocks[idx])
    }
}

impl<'a> FnEmitter<'a> {
    /// Emit a single basic block, then recursively descend into its successors
    /// using a structured-control-flow translation.
    fn emit_block(&mut self, block_id: BlockId) -> Result<(), WasmCodegenError> {
        let idx = self.block_index(block_id)?;
        if self.visited[idx] {
            // Re-entry. For v0.1 we only support straight-line + if/else +
            // simple while; if we hit a re-entry that isn't a loop header
            // already wrapped in a `loop`, we have to give up.
            return Err(WasmCodegenError::unsupported(
                "complex control flow (irreducible CFG); only simple if/else and while supported",
            ));
        }

        // Peek the terminator before emitting instructions so we can detect
        // a while-loop header. For a loop header, the header's *instructions*
        // (which compute the loop condition) must execute inside the wasm
        // `loop` so they re-run on each iteration — NOT before it.
        let is_while = if let Terminator::Branch {
            then_block,
            else_block,
            ..
        } = &self.func.blocks[idx].terminator
        {
            self.is_simple_while_header(block_id, *then_block, *else_block)
        } else if let Terminator::Switch { targets, .. } = &self.func.blocks[idx].terminator {
            self.is_switch_while_header(block_id, targets)
        } else {
            false
        };

        self.visited[idx] = true;
        let block = &self.func.blocks[idx];

        if !is_while {
            // Normal path: emit instructions then dispatch on terminator.
            for inst in &block.instructions {
                self.emit_instruction(inst)?;
            }
        }

        // Snapshot terminator so we can release the borrow when recursing.
        let terminator = block.terminator.clone();
        match terminator {
            Terminator::Return(value) => {
                if let Some(op) = value {
                    self.emit_operand(&op)?;
                }
                self.wfunc.instruction(&W::Return);
            }
            Terminator::Goto(target) => {
                self.emit_block(target)?;
            }
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => {
                if is_while {
                    // Header instructions + branch all run inside the loop body.
                    self.emit_simple_while(block_id, &cond, then_block, else_block)?;
                } else if self.is_inverted_while_header(block_id, then_block, else_block) {
                    // Inverted-while: `if exit_cond { exit } tail_call_back_to_header`
                    self.emit_inverted_while(block_id, &cond, then_block, else_block)?;
                } else {
                    self.emit_if_else(&cond, then_block, else_block)?;
                }
            }
            Terminator::Switch { value, targets, default } => {
                if is_while {
                    self.emit_simple_while_switch(block_id, &value, &targets, default)?;
                } else {
                    self.emit_switch(value, targets, default)?;
                }
            }
            Terminator::Unreachable => {
                self.wfunc.instruction(&W::Unreachable);
            }
        }
        Ok(())
    }

    /// Detect: branch from `header` -> (body, exit) where `body` ends in
    /// Goto(header). That's a simple while loop.
    ///
    /// Also detects the `while true { match ... { Pat => body, _ => break } }`
    /// pattern where `body` terminates in a Switch whose at least one arm
    /// eventually loops back to `header` (via a Goto chain).
    ///
    /// Also detects `while cond { if inner { push } i++ }` where the body
    /// block ends in a Branch (inner if) whose arms eventually reach `header`.
    fn is_simple_while_header(&self, header: BlockId, body: BlockId, _exit: BlockId) -> bool {
        let body_idx = match self.id_to_index.get(&body.0) {
            Some(&i) => i,
            None => return false,
        };
        // Simple case: body directly Goto's header.
        if matches!(
            self.func.blocks[body_idx].terminator,
            Terminator::Goto(b) if b == header
        ) {
            return true;
        }
        // While-true-with-match pattern: body ends in a Switch where at least
        // one arm eventually reaches `header` via a Goto chain.
        if let Terminator::Switch { targets, .. } = &self.func.blocks[body_idx].terminator {
            for (_, bid) in targets {
                if self.goto_chain_reaches(*bid, header) {
                    return true;
                }
            }
        }
        // While-with-inner-if: body ends in Branch where at least one arm
        // eventually reaches header (while loop with an if inside the body).
        if let Terminator::Branch { then_block, else_block, .. } = &self.func.blocks[body_idx].terminator {
            if self.goto_chain_reaches(*then_block, header) || self.goto_chain_reaches(*else_block, header) {
                return true;
            }
        }
        false
    }

    /// Detect: `if exit_cond { return/exit } loop_body_that_Goto_header` — the
    /// "inverted while" produced by tail-call optimization: the ELSE arm (not the
    /// THEN arm) loops back to the function entry. Example:
    ///   `if n <= 0 { return base } return f(n-1, acc)` where `return f(...)` is
    ///   lowered as a Goto-chain back to the function header.
    fn is_inverted_while_header(&self, header: BlockId, _then_block: BlockId, else_block: BlockId) -> bool {
        self.goto_chain_reaches(else_block, header)
    }

    /// Detect: Switch from `header` where one of the arms ends in Goto(header).
    /// Covers `while let Some(v) = expr { ... }` patterns.
    /// Follows linear Goto chains to handle multi-block body layouts from the compiler.
    fn is_switch_while_header(&self, header: BlockId, targets: &[(i64, BlockId)]) -> bool {
        for (_, body) in targets {
            if self.goto_chain_reaches(*body, header) {
                return true;
            }
        }
        false
    }

    /// Returns true if following the linear Goto chain from `start` eventually reaches `target`.
    fn goto_chain_reaches(&self, start: BlockId, target: BlockId) -> bool {
        let mut current = start;
        let mut steps = 0usize;
        loop {
            if current == target { return true; }
            if steps > 32 { return false; } // guard against cycles
            steps += 1;
            let idx = match self.id_to_index.get(&current.0) { Some(&i) => i, None => return false };
            match self.func.blocks[idx].terminator {
                Terminator::Goto(next) => current = next,
                _ => return false,
            }
        }
    }

    /// Emit a Switch-based while loop (e.g. `while let Some(v) = expr`).
    /// Structure: header has Switch terminator; one arm is the body (ends in Goto(header));
    /// the default arm is the exit path.
    fn emit_simple_while_switch(
        &mut self,
        header: BlockId,
        value: &Operand,
        targets: &[(i64, BlockId)],
        default: BlockId,
    ) -> Result<(), WasmCodegenError> {
        // Find the body arm (the one that loops back to header).
        let (match_val, body_id) = targets
            .iter()
            .find(|(_, bid)| {
                let idx = match self.id_to_index.get(&bid.0) { Some(&i) => i, None => return false };
                matches!(self.func.blocks[idx].terminator, Terminator::Goto(b) if b == header)
            })
            .ok_or_else(|| WasmCodegenError::new("switch-while: no body arm found"))?;

        let body_idx = self.block_index(*body_id)?;
        if self.visited[body_idx] {
            return Err(WasmCodegenError::unsupported("loop body already visited (switch-while)"));
        }

        self.wfunc.instruction(&W::Block(BlockType::Empty)); // outer: break = exit
        self.wfunc.instruction(&W::Loop(BlockType::Empty));  // inner: continue

        // Re-run header instructions on each iteration.
        let header_idx = self.block_index(header)?;
        for inst in self.func.blocks[header_idx].instructions.clone() {
            self.emit_instruction(&inst)?;
        }

        // Condition: if tag != match_val, break out.
        self.emit_operand(value)?;
        self.wfunc.instruction(&W::I64Const(*match_val));
        self.wfunc.instruction(&W::I64Eq);   // i32: 1 if matching variant
        self.wfunc.instruction(&W::I32Eqz);  // i32: 1 if NOT matching → exit
        self.wfunc.instruction(&W::BrIf(1)); // break outer block

        // Body: follow the Goto chain from body_id until we reach header (back edge).
        // The compiler may split the body into multiple blocks (e.g. payload-extraction
        // block + computation block), so we must follow linear Goto chains.
        let mut current_body = *body_id;
        loop {
            let curr_idx = self.block_index(current_body)?;
            if self.visited[curr_idx] {
                return Err(WasmCodegenError::unsupported("switch-while body block already visited"));
            }
            self.visited[curr_idx] = true;
            let instrs = self.func.blocks[curr_idx].instructions.clone();
            for inst in &instrs {
                self.emit_instruction(inst)?;
            }
            let term = self.func.blocks[curr_idx].terminator.clone();
            match term {
                Terminator::Goto(b) if b == header => {
                    self.wfunc.instruction(&W::Br(0)); // continue loop
                    break;
                }
                Terminator::Goto(next) => {
                    current_body = next; // follow chain
                }
                Terminator::Return(ref v) => {
                    if let Some(op) = v.as_ref() {
                        self.emit_operand(op)?;
                    }
                    self.wfunc.instruction(&W::Return);
                    break;
                }
                _ => return Err(WasmCodegenError::unsupported(
                    "switch-while body must end in Goto(header) or Return",
                )),
            }
        }

        self.wfunc.instruction(&W::End); // end loop
        self.wfunc.instruction(&W::End); // end outer block

        // Continue with the exit/default path.
        self.emit_block(default)?;
        Ok(())
    }

    /// Emit a simple while loop:
    ///
    /// ```text
    /// loop {
    ///   <cond>
    ///   br_if exit_label
    ///   <body>
    ///   br loop_label
    /// }
    /// <continuation: exit block>
    /// ```
    fn emit_simple_while(
        &mut self,
        header: BlockId,
        cond: &Operand,
        body: BlockId,
        exit: BlockId,
    ) -> Result<(), WasmCodegenError> {
        // Mark the body as visited (we'll inline it inside the loop), and let
        // the header's "already visited" check prevent recursion when the body
        // does Goto(header).
        let body_idx = self.block_index(body)?;
        if self.visited[body_idx] {
            return Err(WasmCodegenError::unsupported(
                "loop body already visited (irreducible CFG)",
            ));
        }

        self.wfunc.instruction(&W::Block(BlockType::Empty));
        self.wfunc.instruction(&W::Loop(BlockType::Empty));

        // FIRST: re-run the header block's instructions (they compute the
        // loop condition on each iteration).
        let header_idx = self.block_index(header)?;
        for inst in self.func.blocks[header_idx].instructions.clone() {
            self.emit_instruction(&inst)?;
        }

        // Then evaluate the condition + branch out if false.
        // Kryos booleans are i64 (0 = false, anything else = true).
        // WASM's `i32.eqz` / `br_if` work on i32, so wrap.
        self.emit_operand(cond)?;
        self.wfunc.instruction(&W::I32WrapI64);
        self.wfunc.instruction(&W::I32Eqz);
        self.wfunc.instruction(&W::BrIf(1)); // break out of outer block

        // Body: emit body block's instructions, ignoring its Goto(header)
        // terminator (we replace it with `br 0` to loop back).
        self.visited[body_idx] = true;
        let body_block = &self.func.blocks[body_idx];
        for inst in &body_block.instructions {
            self.emit_instruction(inst)?;
        }
        // body should terminate in Goto(header) — that's the loop edge.
        // Also handle Switch (while-true with inner match, e.g. `while let`).
        match body_block.terminator.clone() {
            Terminator::Goto(b) if b == header => {
                self.wfunc.instruction(&W::Br(0)); // continue
            }
            Terminator::Return(value) => {
                // Early return inside loop body.
                if let Some(op) = &value {
                    self.emit_operand(op)?;
                }
                self.wfunc.instruction(&W::Return);
            }
            Terminator::Switch { value, targets, default } => {
                // `while true { match expr { Pat => body, _ => break } }` pattern.
                // We're inside: block(outer) { loop { ... [body instrs] [here] } }
                // Emit each matching arm as an `if`; arm back-edge = br 1 (loop),
                // fall-through default = br 1 (outer block = exit).
                self.emit_while_body_switch(&value, &targets, default, header, exit)?;
            }
            _ => {
                return Err(WasmCodegenError::unsupported(
                    "while-loop body must end in Goto(header) or Return",
                ));
            }
        }

        self.wfunc.instruction(&W::End); // end loop
        self.wfunc.instruction(&W::End); // end outer block

        // Continue with the exit block.
        self.emit_block(exit)?;
        Ok(())
    }

    /// Emit the body of a `while true { match expr { Pat => ..., _ => break } }` loop.
    ///
    /// Called from `emit_simple_while` when the body block terminates in a Switch.
    /// We are already inside:
    ///   block(outer)  ; br 1 from loop body = exit
    ///     loop        ; br 0 from loop body = continue
    ///       [header instructions already emitted]
    ///       [condition check already emitted]
    ///       [body_bb instructions already emitted]
    ///       [we are HERE — body terminated in Switch]
    ///
    /// For each matching arm we emit `if (tag == val) { arm_instrs; br 1 } end`
    /// where inside the if: br 0 exits the if, br 1 continues the loop, br 2 exits.
    /// After all ifs, we emit `br 1` (exit outer block = break the loop).
    fn emit_while_body_switch(
        &mut self,
        value: &Operand,
        targets: &[(i64, BlockId)],
        default: BlockId,
        loop_header: BlockId,
        loop_exit: BlockId,
    ) -> Result<(), WasmCodegenError> {
        // Group targets by arm block (or-patterns share one block).
        let mut seen_bids: Vec<u32> = Vec::new();
        let mut bid_vals: HashMap<u32, (BlockId, Vec<i64>)> = HashMap::new();
        for (val, bid) in targets {
            if let Some(entry) = bid_vals.get_mut(&bid.0) {
                entry.1.push(*val);
            } else {
                seen_bids.push(bid.0);
                bid_vals.insert(bid.0, (*bid, vec![*val]));
            }
        }

        // Emit one `if` per unique arm.
        for &bid_u32 in &seen_bids {
            let (bid, vals) = &bid_vals[&bid_u32];
            self.emit_switch_cond(value, vals)?;
            self.wfunc.instruction(&W::If(BlockType::Empty));
            // Inside the if: depth 0=if, 1=loop, 2=outer block.

            let arm_idx = self.block_index(*bid)?;
            if !self.visited[arm_idx] {
                self.visited[arm_idx] = true;
                let instrs: Vec<_> = self.func.blocks[arm_idx].instructions.clone();
                for inst in &instrs {
                    self.emit_instruction(inst)?;
                }
                let arm_term = self.func.blocks[arm_idx].terminator.clone();
                match arm_term {
                    Terminator::Goto(b) if b == loop_header => {
                        self.wfunc.instruction(&W::Br(1)); // continue loop
                    }
                    Terminator::Goto(b) if b == loop_exit => {
                        self.wfunc.instruction(&W::Br(2)); // exit loop
                    }
                    Terminator::Goto(b) if self.goto_chain_reaches(b, loop_header) => {
                        // Multi-block arm that eventually loops back; inline chain.
                        let mut cur = b;
                        loop {
                            let ci = self.block_index(cur)?;
                            if self.visited[ci] {
                                self.wfunc.instruction(&W::Br(1));
                                break;
                            }
                            self.visited[ci] = true;
                            let ci_instrs: Vec<_> = self.func.blocks[ci].instructions.clone();
                            for inst in &ci_instrs { self.emit_instruction(inst)?; }
                            match self.func.blocks[ci].terminator.clone() {
                                Terminator::Goto(n) if n == loop_header => {
                                    self.wfunc.instruction(&W::Br(1)); // continue
                                    break;
                                }
                                Terminator::Goto(n) => { cur = n; }
                                Terminator::Return(ref v) => {
                                    if let Some(op) = v.as_ref() { self.emit_operand(op)?; }
                                    self.wfunc.instruction(&W::Return);
                                    break;
                                }
                                _ => { self.wfunc.instruction(&W::Unreachable); break; }
                            }
                        }
                    }
                    Terminator::Return(ref v) => {
                        if let Some(op) = v.as_ref() { self.emit_operand(op)?; }
                        self.wfunc.instruction(&W::Return);
                    }
                    _ => { self.wfunc.instruction(&W::Unreachable); }
                }
            }
            self.wfunc.instruction(&W::End); // end if
        }

        // Default arm (break/fallthrough): mark visited, emit its instructions.
        let default_idx = self.block_index(default)?;
        if !self.visited[default_idx] {
            self.visited[default_idx] = true;
            let def_instrs: Vec<_> = self.func.blocks[default_idx].instructions.clone();
            for inst in &def_instrs { self.emit_instruction(inst)?; }
            // Terminator of default is typically Goto(loop_exit) = break.
            // We DON'T emit it; the `br 1` below handles the exit.
        }

        // Fall-through after all ifs = no arm matched = break the loop.
        // At body depth (inside loop, outside any if): br 1 = exit outer block.
        self.wfunc.instruction(&W::Br(1));

        Ok(())
    }

    /// Emit an if/else: detect the join point (the first block both arms
    /// reach), emit then/else inside a wasm `if`, then continue with join.
    fn emit_if_else(
        &mut self,
        cond: &Operand,
        then_block: BlockId,
        else_block: BlockId,
    ) -> Result<(), WasmCodegenError> {
        let join = find_common_join(self.func, &self.id_to_index, then_block, else_block);
        // Short-circuit `&&`: the else branch IS the join (e.g. bb_join = return block).
        // Don't put it inside the wasm `else`; let the then arm fall through to it after `end`.
        let else_is_join = join.map(|j| j == else_block).unwrap_or(false);

        self.emit_operand(cond)?;
        self.wfunc.instruction(&W::I32WrapI64);
        self.wfunc.instruction(&W::If(BlockType::Empty));

        // then arm
        self.emit_arm(then_block, join)?;

        if !else_is_join {
            self.wfunc.instruction(&W::Else);
            self.emit_arm(else_block, join)?;
        }

        self.wfunc.instruction(&W::End);

        // Continue with the join block (if any).
        if let Some(j) = join {
            let j_idx = self.block_index(j)?;
            if !self.visited[j_idx] {
                self.emit_block(j)?;
            }
        }
        Ok(())
    }

    /// Emit one arm of an if/else: the block's instructions, but stop before
    /// emitting a Goto to the join point (the `if` block in WASM handles the
    /// continuation).
    fn emit_arm(&mut self, arm: BlockId, join: Option<BlockId>) -> Result<(), WasmCodegenError> {
        let idx = self.block_index(arm)?;
        if self.visited[idx] {
            return Err(WasmCodegenError::unsupported(
                "if-arm shares a block with another path",
            ));
        }
        self.visited[idx] = true;
        let block = &self.func.blocks[idx];
        for inst in &block.instructions {
            self.emit_instruction(inst)?;
        }
        // Snapshot terminator so we can release the borrow on `self`.
        let terminator = block.terminator.clone();
        match terminator {
            Terminator::Goto(t) if Some(t) == join => {
                // Fall through — the outer `if` continues at the join.
            }
            Terminator::Return(ref v) => {
                if let Some(op) = v.as_ref() {
                    self.emit_operand(op)?;
                }
                self.wfunc.instruction(&W::Return);
            }
            Terminator::Goto(t) => {
                // Tail-call into another block within this arm.
                self.emit_block(t)?;
            }
            Terminator::Branch {
                ref cond,
                then_block,
                else_block,
            } => {
                // Nested if/else inside an arm (chained `elif`). Emit it
                // recursively, sharing the *outer* join point so all arms
                // converge to the same wasm continuation.
                self.emit_if_else_with_join(cond, then_block, else_block, join)?;
            }
            Terminator::Switch { .. } | Terminator::Unreachable => {
                self.wfunc.instruction(&W::Unreachable);
            }
        }
        Ok(())
    }

    /// Like `emit_if_else` but uses a caller-supplied join (used when emitting
    /// nested arms inside an outer if/else).
    fn emit_if_else_with_join(
        &mut self,
        cond: &Operand,
        then_block: BlockId,
        else_block: BlockId,
        join: Option<BlockId>,
    ) -> Result<(), WasmCodegenError> {
        let else_is_join = join.map(|j| j == else_block).unwrap_or(false);
        self.emit_operand(cond)?;
        self.wfunc.instruction(&W::I32WrapI64);
        self.wfunc.instruction(&W::If(BlockType::Empty));
        self.emit_arm(then_block, join)?;
        if !else_is_join {
            self.wfunc.instruction(&W::Else);
            self.emit_arm(else_block, join)?;
        }
        self.wfunc.instruction(&W::End);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Instruction-level lowering
    // -----------------------------------------------------------------------

    fn emit_instruction(&mut self, inst: &Instruction) -> Result<(), WasmCodegenError> {
        match inst {
            Instruction::Assign { dest, value } => {
                // If the assigned local is `void`-typed, treat this as a
                // statement: emit the rvalue for its side effect and skip
                // the LocalSet (the call leaves nothing on the stack).
                let dest_is_void = self
                    .func
                    .locals
                    .get(dest.0 as usize)
                    .map(|l| is_void(&l.ty))
                    .unwrap_or(false);
                if dest_is_void {
                    self.emit_rvalue(value)?;
                } else {
                    self.emit_rvalue(value)?;
                    self.wfunc.instruction(&W::LocalSet(dest.0));
                }
            }
            Instruction::Nop | Instruction::DebugLine(_) => {}
            Instruction::ArcRetain { .. }
            | Instruction::ArcRelease { .. }
            | Instruction::Drop { .. } => {
                // In v0.1 we have no heap, so ARC ops are no-ops.
            }
            Instruction::StoreField { object, field, value } => {
                let struct_name = match self.operand_ty(object) {
                    Some(MirType::Struct(n)) => n,
                    // Array element stores: MIR lowers `arr[i].field = v` as
                    // StoreField { object: Ptr(Struct(name)), ... } so the LLVM
                    // backend can emit an inttoptr GEP. In WASM, Ptr is an i64
                    // handle; extract the struct name and treat the same way.
                    Some(MirType::Ptr(inner)) => match inner.as_ref() {
                        MirType::Struct(n) => n.clone(),
                        _ => return Err(WasmCodegenError::unsupported(
                            "StoreField on non-struct pointer",
                        )),
                    },
                    _ => return Err(WasmCodegenError::unsupported(
                        "StoreField on non-struct operand",
                    )),
                };
                let field_name = field.clone();
                let (field_idx, field_ty) = self.cg.struct_defs
                    .get(&struct_name)
                    .and_then(|fs| {
                        fs.iter().enumerate()
                            .find(|(_, (n, _))| n == &field_name)
                            .map(|(i, (_, t))| (i as i32, t.clone()))
                    })
                    .unwrap_or((0, MirType::I64));
                let array_set_idx = self.cg.array_set_idx;
                self.emit_operand(object)?;
                self.wfunc.instruction(&W::I32Const(field_idx));
                self.emit_operand(value)?;
                // f64 fields must be stored as i64 bit-patterns.
                if matches!(field_ty, MirType::F64) {
                    self.wfunc.instruction(&W::I64ReinterpretF64);
                }
                self.wfunc.instruction(&W::Call(array_set_idx));
            }
            Instruction::StoreDeref { .. }
            | Instruction::Spawn { .. }
            | Instruction::Send { .. }
            | Instruction::Receive { .. }
            | Instruction::ActorSpawn { .. }
            | Instruction::ActorSend { .. }
            | Instruction::ActorStateLoad { .. }
            | Instruction::ActorStateStore { .. } => {
                return Err(WasmCodegenError::unsupported(
                    "heap operations / spawn / channels / actors (v0.1 is compute-only)",
                ));
            }
        }
        Ok(())
    }

    fn emit_rvalue(&mut self, rv: &RValue) -> Result<(), WasmCodegenError> {
        match rv {
            RValue::Use(op) => self.emit_operand(op)?,
            RValue::ConstInt(v) => {
                self.wfunc.instruction(&W::I64Const(*v));
            }
            RValue::ConstFloat(v) => {
                self.wfunc.instruction(&W::F64Const(*v));
            }
            RValue::ConstBool(b) => {
                self.wfunc.instruction(&W::I64Const(if *b { 1 } else { 0 }));
            }
            RValue::ConstString(s) => {
                // v0.2: push a single packed i64 value with low32 = offset,
                // high32 = length. This makes strings first-class — they
                // survive in locals, parameters, and returns without a side
                // table, and `len(s)` is just a shift.
                let (offset, len) = self.cg.intern_string(s);
                let packed = ((len as u64) << 32) | (offset as u64);
                self.wfunc.instruction(&W::I64Const(packed as i64));
            }
            RValue::ConstNone => {
                self.wfunc.instruction(&W::I64Const(0));
            }
            RValue::BinOp { op, left, right } => {
                let lty = self.operand_ty(left).unwrap_or(MirType::I64);
                let rty = self.operand_ty(right).unwrap_or(MirType::I64);
                if matches!(op, MirBinOp::Add)
                    && matches!(lty, MirType::Str)
                    && matches!(rty, MirType::Str)
                {
                    self.emit_operand(left)?;
                    self.emit_unpack_string();
                    self.emit_operand(right)?;
                    self.emit_unpack_string();
                    self.wfunc.instruction(&W::Call(self.cg.string_concat_idx));
                } else if matches!(lty, MirType::F64 | MirType::F32)
                    || matches!(rty, MirType::F64 | MirType::F32)
                {
                    self.emit_operand(left)?;
                    self.emit_operand(right)?;
                    self.emit_binop_f64(*op)?;
                } else {
                    self.emit_operand(left)?;
                    self.emit_operand(right)?;
                    self.emit_binop(*op)?;
                }
            }
            RValue::StringConcat(parts) => {
                self.emit_string_concat(parts)?;
            }
            RValue::UnOp { op, operand } => {
                match op {
                    MirUnOp::Neg => {
                        let ty = self.operand_ty(operand).unwrap_or(MirType::I64);
                        if matches!(ty, MirType::F64 | MirType::F32) {
                            self.emit_operand(operand)?;
                            if matches!(ty, MirType::F32) {
                                self.wfunc.instruction(&W::F64PromoteF32);
                            }
                            self.wfunc.instruction(&W::F64Neg);
                        } else {
                            // Integer negate: 0 - x; use scratch local to preserve stack order.
                            self.emit_operand(operand)?;
                            let scratch = self.cg_scratch_local();
                            self.wfunc.instruction(&W::LocalSet(scratch));
                            self.wfunc.instruction(&W::I64Const(0));
                            self.wfunc.instruction(&W::LocalGet(scratch));
                            self.wfunc.instruction(&W::I64Sub);
                        }
                    }
                    _ => {
                        self.emit_operand(operand)?;
                        self.emit_unop(*op)?;
                    }
                }
            }
            RValue::Call { func, args } => {
                self.emit_call(func, args)?;
            }
            RValue::Cast { operand, ty } => {
                let src_ty = self.operand_ty(operand).unwrap_or(MirType::I64);
                self.emit_operand(operand)?;
                // f64/f32 source requires explicit truncation to integer.
                if matches!(src_ty, MirType::F64 | MirType::F32) {
                    if matches!(src_ty, MirType::F32) {
                        self.wfunc.instruction(&W::F64PromoteF32);
                    }
                    match ty {
                        MirType::I64 | MirType::U64 => {
                            self.wfunc.instruction(&W::I64TruncF64S);
                        }
                        MirType::I32 | MirType::U32 => {
                            self.wfunc.instruction(&W::I32TruncF64S);
                            self.wfunc.instruction(&W::I64ExtendI32S);
                        }
                        MirType::F64 => { /* f32->f64 already promoted, f64->f64 no-op */ }
                        _ => { self.emit_cast(ty)?; }
                    }
                } else {
                    self.emit_cast(ty)?;
                }
            }
            // -------------------------------------------------------------
            // WASM v0.3: Array literal — `[a, b, c]`
            // Lowers to:  arr = array_new(n); array_set(arr, 0, a); ...; arr
            // -------------------------------------------------------------
            RValue::Array(elems) => {
                // Push count, call array_new -> i64 packed array handle.
                self.wfunc.instruction(&W::I32Const(elems.len() as i32));
                self.wfunc.instruction(&W::Call(self.cg.array_new_idx));
                // Stash the array handle in scratch so we can re-use it.
                let scratch = self.cg_scratch_local();
                self.wfunc.instruction(&W::LocalSet(scratch));
                // For each element: load array, push idx, push value, set.
                for (i, elem) in elems.iter().enumerate() {
                    self.wfunc.instruction(&W::LocalGet(scratch));
                    self.wfunc.instruction(&W::I32Const(i as i32));
                    self.emit_operand(elem)?;
                    self.wfunc.instruction(&W::Call(self.cg.array_set_idx));
                }
                // Leave the array handle on the stack as the rvalue result.
                self.wfunc.instruction(&W::LocalGet(scratch));
            }
            // -------------------------------------------------------------
            // WASM v0.3: Array indexing — `arr[i]`
            // -------------------------------------------------------------
            RValue::Index { object, index } => {
                self.emit_operand(object)?;            // packed i64 array
                self.emit_operand(index)?;             // index i64
                self.wfunc.instruction(&W::I32WrapI64);// -> i32 index
                self.wfunc.instruction(&W::Call(self.cg.array_get_idx));
            }
            // -----------------------------------------------------------------
            // WASM v0.4: Struct literal — `Foo { a: x, b: y }`
            // Represent structs as i64 array handles: alloc N slots, fill each.
            // -----------------------------------------------------------------
            RValue::Struct { name, fields } => {
                let n = fields.len() as i32;
                self.wfunc.instruction(&W::I32Const(n));
                self.wfunc.instruction(&W::Call(self.cg.array_new_idx));
                let scratch = self.cg_scratch_local();
                self.wfunc.instruction(&W::LocalSet(scratch));
                // Resolve field order and types from struct_defs for stable slot indices.
                let struct_name = name.clone();
                let field_defs: Vec<(String, MirType)> = self.cg.struct_defs
                    .get(&struct_name)
                    .cloned()
                    .unwrap_or_else(|| fields.iter().map(|(n, _)| (n.clone(), MirType::I64)).collect());
                let array_set_idx = self.cg.array_set_idx;
                for (fname, fval) in fields {
                    let (idx, field_ty) = field_defs.iter().enumerate()
                        .find(|(_, (n, _))| n == fname)
                        .map(|(i, (_, t))| (i as i32, t.clone()))
                        .unwrap_or((0, MirType::I64));
                    self.wfunc.instruction(&W::LocalGet(scratch));
                    self.wfunc.instruction(&W::I32Const(idx));
                    self.emit_operand(fval)?;
                    // f64 fields must be stored as i64 bit-patterns.
                    if matches!(field_ty, MirType::F64) {
                        self.wfunc.instruction(&W::I64ReinterpretF64);
                    }
                    self.wfunc.instruction(&W::Call(array_set_idx));
                }
                self.wfunc.instruction(&W::LocalGet(scratch));
            }
            // -----------------------------------------------------------------
            // WASM v0.4: Struct field read — `obj.field`
            // -----------------------------------------------------------------
            RValue::Field { object, field } => {
                match self.operand_ty(object) {
                    Some(MirType::Tuple(elem_types)) => {
                        // Tuple field access: field name is "0", "1", ... (numeric index as string).
                        let idx = field.parse::<i32>().unwrap_or(0);
                        let field_ty = elem_types.get(idx as usize).cloned().unwrap_or(MirType::I64);
                        let array_get_idx = self.cg.array_get_idx;
                        self.emit_operand(object)?;
                        self.wfunc.instruction(&W::I32Const(idx));
                        self.wfunc.instruction(&W::Call(array_get_idx));
                        if matches!(field_ty, MirType::F64) {
                            self.wfunc.instruction(&W::F64ReinterpretI64);
                        }
                    }
                    Some(MirType::Struct(struct_name)) => {
                        let field_name = field.clone();
                        let (field_idx, field_ty) = self.cg.struct_defs
                            .get(&struct_name)
                            .and_then(|fs| {
                                fs.iter().enumerate()
                                    .find(|(_, (n, _))| n == &field_name)
                                    .map(|(i, (_, t))| (i as i32, t.clone()))
                            })
                            .unwrap_or((0, MirType::I64));
                        let array_get_idx = self.cg.array_get_idx;
                        self.emit_operand(object)?;
                        self.wfunc.instruction(&W::I32Const(field_idx));
                        self.wfunc.instruction(&W::Call(array_get_idx));
                        // f64 fields are stored as their i64 bit-pattern; reinterpret on read.
                        if matches!(field_ty, MirType::F64) {
                            self.wfunc.instruction(&W::F64ReinterpretI64);
                        }
                    }
                    _ => return Err(WasmCodegenError::unsupported(&format!(
                        "field access `{field}` on non-struct/tuple operand"
                    ))),
                }
            }
            // -----------------------------------------------------------------
            // WASM v2.4: Enum variant construction
            // Encoding: [tag:i64, field0:i64, field1:i64, ...]
            //   slot 0 = variant_idx (the tag)
            //   slots 1..n = payload fields
            // -----------------------------------------------------------------
            RValue::EnumVariant { enum_name: _, variant_idx, fields } => {
                let n_slots = 1 + fields.len();
                self.wfunc.instruction(&W::I32Const(n_slots as i32));
                self.wfunc.instruction(&W::Call(self.cg.array_new_idx));
                let scratch = self.cg_scratch_local();
                self.wfunc.instruction(&W::LocalSet(scratch));
                // Store tag at slot 0.
                self.wfunc.instruction(&W::LocalGet(scratch));
                self.wfunc.instruction(&W::I32Const(0));
                self.wfunc.instruction(&W::I64Const(*variant_idx as i64));
                let array_set_idx = self.cg.array_set_idx;
                self.wfunc.instruction(&W::Call(array_set_idx));
                // Store each payload field at slots 1, 2, ...
                for (i, field_op) in fields.iter().enumerate() {
                    self.wfunc.instruction(&W::LocalGet(scratch));
                    self.wfunc.instruction(&W::I32Const((i + 1) as i32));
                    self.emit_operand(field_op)?;
                    self.wfunc.instruction(&W::Call(array_set_idx));
                }
                self.wfunc.instruction(&W::LocalGet(scratch));
            }
            // -----------------------------------------------------------------
            // WASM v2.4: Enum tag extraction (for Switch dispatch)
            // -----------------------------------------------------------------
            RValue::EnumTag { operand } => {
                self.emit_operand(operand)?;
                self.wfunc.instruction(&W::I32Const(0));
                self.wfunc.instruction(&W::Call(self.cg.array_get_idx));
            }
            // -----------------------------------------------------------------
            // WASM v2.4: Enum payload field extraction
            // -----------------------------------------------------------------
            RValue::EnumPayload { operand, enum_name: _, variant_idx: _, field_idx } => {
                self.emit_operand(operand)?;
                self.wfunc.instruction(&W::I32Const((*field_idx + 1) as i32));
                self.wfunc.instruction(&W::Call(self.cg.array_get_idx));
            }
            // -----------------------------------------------------------------
            // WASM v2.5: Tuple construction — `(a, b, c)`
            // Identical to array/struct: allocate N slots, fill each, leave handle.
            // -----------------------------------------------------------------
            RValue::Tuple(elems) => {
                self.wfunc.instruction(&W::I32Const(elems.len() as i32));
                self.wfunc.instruction(&W::Call(self.cg.array_new_idx));
                let scratch = self.cg_scratch_local();
                self.wfunc.instruction(&W::LocalSet(scratch));
                let array_set_idx = self.cg.array_set_idx;
                for (i, elem) in elems.iter().enumerate() {
                    self.wfunc.instruction(&W::LocalGet(scratch));
                    self.wfunc.instruction(&W::I32Const(i as i32));
                    self.emit_operand(elem)?;
                    self.wfunc.instruction(&W::Call(array_set_idx));
                }
                self.wfunc.instruction(&W::LocalGet(scratch));
            }
            other => {
                return Err(WasmCodegenError::unsupported(&format!(
                    "rvalue `{}` (v0.3 supports scalars, calls, casts, arrays, indexing)",
                    debug_short(other)
                )));
            }
        }
        Ok(())
    }

    fn emit_operand(&mut self, op: &Operand) -> Result<(), WasmCodegenError> {
        match op {
            Operand::Local(id) => {
                self.wfunc.instruction(&W::LocalGet(id.0));
            }
            Operand::Constant(c) => match c {
                Constant::Int(v) => {
                    self.wfunc.instruction(&W::I64Const(*v));
                }
                Constant::Float(v) => {
                    self.wfunc.instruction(&W::F64Const(*v));
                }
                Constant::Bool(b) => {
                    self.wfunc.instruction(&W::I64Const(if *b { 1 } else { 0 }));
                }
                Constant::Str(s) => {
                    let (offset, len) = self.cg.intern_string(s);
                    let packed = ((len as u64) << 32) | (offset as u64);
                    self.wfunc.instruction(&W::I64Const(packed as i64));
                }
                Constant::None => {
                    self.wfunc.instruction(&W::I64Const(0));
                }
            },
        }
        Ok(())
    }

    fn emit_binop(&mut self, op: MirBinOp) -> Result<(), WasmCodegenError> {
        // For v0.1 we operate on i64 by default. Float ops would need a type
        // tag we don't have yet at the rvalue level — the MIR doesn't currently
        // distinguish "f64 add" from "i64 add" in the op variant, it relies on
        // the lowering knowing the operand types. We'll improve this later.
        let inst = match op {
            MirBinOp::Add => W::I64Add,
            MirBinOp::Sub => W::I64Sub,
            MirBinOp::Mul => W::I64Mul,
            MirBinOp::Div => W::I64DivS,
            MirBinOp::Mod => W::I64RemS,
            MirBinOp::Pow => {
                return Err(WasmCodegenError::unsupported(
                    "integer exponent (use a loop or import a helper for v0.1)",
                ));
            }
            MirBinOp::Eq => W::I64Eq,
            MirBinOp::Neq => W::I64Ne,
            MirBinOp::Lt => W::I64LtS,
            MirBinOp::Gt => W::I64GtS,
            MirBinOp::LtEq => W::I64LeS,
            MirBinOp::GtEq => W::I64GeS,
            MirBinOp::And => W::I64And,
            MirBinOp::Or => W::I64Or,
            MirBinOp::BitAnd => W::I64And,
            MirBinOp::BitOr => W::I64Or,
            MirBinOp::BitXor => W::I64Xor,
            MirBinOp::Shl => W::I64Shl,
            MirBinOp::Shr => W::I64ShrS,
        };
        self.wfunc.instruction(&inst);
        // WASM comparison ops return i32, but our value model is i64. Extend.
        if matches!(
            op,
            MirBinOp::Eq
                | MirBinOp::Neq
                | MirBinOp::Lt
                | MirBinOp::Gt
                | MirBinOp::LtEq
                | MirBinOp::GtEq
        ) {
            self.wfunc.instruction(&W::I64ExtendI32U);
        }
        Ok(())
    }

    fn emit_unop(&mut self, op: MirUnOp) -> Result<(), WasmCodegenError> {
        match op {
            MirUnOp::Neg => {
                // Handled in emit_rvalue UnOp arm (needs type info + scratch local).
                // This path should not be reached.
                unreachable!("MirUnOp::Neg must be handled in emit_rvalue");
            }
            MirUnOp::Not => {
                // logical not: x == 0
                self.wfunc.instruction(&W::I64Eqz);
                self.wfunc.instruction(&W::I64ExtendI32U);
            }
            MirUnOp::BitNot => {
                self.wfunc.instruction(&W::I64Const(-1));
                self.wfunc.instruction(&W::I64Xor);
            }
        }
        Ok(())
    }

    /// Get the MIR type of an operand for codegen dispatch (println/len/etc).
    fn operand_ty(&self, op: &Operand) -> Option<MirType> {
        match op {
            Operand::Local(id) => self
                .func
                .locals
                .iter()
                .find(|l| l.id.0 == id.0)
                .map(|l| l.ty.clone()),
            Operand::Constant(c) => Some(match c {
                Constant::Int(_) => MirType::I64,
                Constant::Float(_) => MirType::F64,
                Constant::Bool(_) => MirType::Bool,
                Constant::Str(_) => MirType::Str,
                Constant::None => MirType::Void,
            }),
        }
    }

    /// Emit code that unpacks a packed-string i64 (on the stack) into two
    /// i32 values (offset, len) on the stack. Consumes the i64.
    fn emit_unpack_string(&mut self) {
        // Stack: [packed:i64]
        // We want: [offset:i32, len:i32]
        // Use a temp local in the wfunc to dup the value. But we don't have
        // a way to allocate fresh locals here — instead, use the wasm
        // pattern: dup via local.tee/local.get if a scratch i64 local exists,
        // OR use bit math without dup by computing both halves with one
        // load via i32 wrap and shr+wrap. Simplest: TEE into scratch.
        //
        // We use the LAST declared local as a scratch i64. To make that
        // available unconditionally we always reserve one extra i64 local
        // at the end of every function (see emit_function).
        let scratch = self.cg_scratch_local();
        // [packed]
        self.wfunc.instruction(&W::LocalTee(scratch));
        // [packed]
        self.wfunc.instruction(&W::I32WrapI64);
        // [offset:i32]
        self.wfunc.instruction(&W::LocalGet(scratch));
        self.wfunc.instruction(&W::I64Const(32));
        self.wfunc.instruction(&W::I64ShrU);
        self.wfunc.instruction(&W::I32WrapI64);
        // [offset:i32, len:i32]
    }

    /// Return the WASM local index of the scratch i64 local appended at
    /// the very end of every function (see emit_function).
    fn cg_scratch_local(&self) -> u32 {
        // We reserve 1 extra i64 local after all user locals.
        // WASM local index = n_params + (user_locals - n_params) + 0
        //                  = func.locals.len()
        self.func.locals.len() as u32
    }

    fn emit_call(&mut self, func: &str, args: &[Operand]) -> Result<(), WasmCodegenError> {
        // -------------------------------------------------------------
        // Built-in: println / print
        // -------------------------------------------------------------
        if func == "println" || func == "print" {
            if args.len() == 1 {
                let ty = self.operand_ty(&args[0]).unwrap_or(MirType::I64);
                match (&args[0], &ty) {
                    // String constant: emit (offset,len) directly, no unpack.
                    (Operand::Constant(Constant::Str(s)), _) => {
                        let (offset, len) = self.cg.intern_string(s);
                        self.wfunc.instruction(&W::I32Const(offset as i32));
                        self.wfunc.instruction(&W::I32Const(len as i32));
                        self.wfunc.instruction(&W::Call(self.cg.print_str_idx));
                        return Ok(());
                    }
                    // String-typed local (or any string operand): unpack the
                    // packed i64 into (offset, len) then call print_str.
                    (op, MirType::Str) => {
                        self.emit_operand(op)?;
                        self.emit_unpack_string();
                        self.wfunc.instruction(&W::Call(self.cg.print_str_idx));
                        return Ok(());
                    }
                    // Float-typed operand: print as f64.
                    (op, MirType::F64) | (op, MirType::F32) => {
                        self.emit_operand(op)?;
                        if matches!(ty, MirType::F32) {
                            self.wfunc.instruction(&W::F64PromoteF32);
                        }
                        self.wfunc.instruction(&W::Call(self.cg.print_f64_idx));
                        return Ok(());
                    }
                    // Default: print as i64.
                    (op, _) => {
                        self.emit_operand(op)?;
                        self.wfunc.instruction(&W::Call(self.cg.print_i64_idx));
                        return Ok(());
                    }
                }
            }
            return Err(WasmCodegenError::unsupported(
                "println with multiple arguments (concat strings with `str_concat` first)",
            ));
        }

        // -------------------------------------------------------------
        // Built-in: len(s/arr) — string or array length
        // Strings: high 32 bits of packed i64 (offset|len encoding).
        // Arrays: opaque i64 handles — call host kryos_array_length.
        // -------------------------------------------------------------
        if func == "len" && args.len() == 1 {
            let ty = self.operand_ty(&args[0]).unwrap_or(MirType::Str);
            self.emit_operand(&args[0])?;
            match ty {
                MirType::Array(_, _) => {
                    let idx = self.cg.array_length_idx;
                    self.wfunc.instruction(&W::Call(idx));
                    self.wfunc.instruction(&W::I64ExtendI32U);
                }
                _ => {
                    self.wfunc.instruction(&W::I64Const(32));
                    self.wfunc.instruction(&W::I64ShrU);
                }
            }
            return Ok(());
        }

        // -------------------------------------------------------------
        // Built-in: str_concat(a, b) — concatenate two strings
        // -------------------------------------------------------------
        if func == "str_concat" && args.len() == 2 {
            // Push (off1, len1, off2, len2) by unpacking each packed i64.
            // We have one scratch i64 local; reuse it sequentially.
            for arg in args {
                self.emit_operand(arg)?;
                self.emit_unpack_string();
            }
            self.wfunc.instruction(&W::Call(self.cg.string_concat_idx));
            return Ok(());
        }

        // -------------------------------------------------------------
        // Built-in: array_new(count) — allocate i64 array of given size
        // -------------------------------------------------------------
        if func == "array_new" && args.len() == 1 {
            self.emit_operand(&args[0])?;
            self.wfunc.instruction(&W::I32WrapI64);
            self.wfunc.instruction(&W::Call(self.cg.array_new_idx));
            return Ok(());
        }

        // -------------------------------------------------------------
        // Built-in: array_get(arr, idx) -> i64
        // -------------------------------------------------------------
        if func == "array_get" && args.len() == 2 {
            self.emit_operand(&args[0])?;            // packed i64 array
            self.emit_operand(&args[1])?;            // index i64
            self.wfunc.instruction(&W::I32WrapI64);  // -> i32 index
            self.wfunc.instruction(&W::Call(self.cg.array_get_idx));
            return Ok(());
        }

        // -------------------------------------------------------------
        // Built-in: array_set(arr, idx, value)
        // -------------------------------------------------------------
        if func == "array_set" && args.len() == 3 {
            self.emit_operand(&args[0])?;            // packed i64 array
            self.emit_operand(&args[1])?;            // index i64
            self.wfunc.instruction(&W::I32WrapI64);  // -> i32 index
            self.emit_operand(&args[2])?;            // value i64
            self.wfunc.instruction(&W::Call(self.cg.array_set_idx));
            return Ok(());
        }

        // -------------------------------------------------------------
        // WASM v0.4 web builtins — DOM / canvas / fetch / alert
        //   dom_set_text(id: str, text: str)
        //   dom_get_value(id: str) -> str
        //   alert(msg: str)
        //   canvas_fill_rect(canvas_id, x, y, w, h, color)
        //   canvas_clear(canvas_id)
        //   fetch_text(url: str) -> str
        // -------------------------------------------------------------
        if func == "dom_set_text" && args.len() == 2 {
            self.emit_operand(&args[0])?; self.emit_unpack_string();
            self.emit_operand(&args[1])?; self.emit_unpack_string();
            self.wfunc.instruction(&W::Call(self.cg.dom_set_text_idx));
            return Ok(());
        }
        if func == "dom_get_value" && args.len() == 1 {
            self.emit_operand(&args[0])?; self.emit_unpack_string();
            self.wfunc.instruction(&W::Call(self.cg.dom_get_value_idx));
            return Ok(());
        }
        if func == "alert" && args.len() == 1 {
            self.emit_operand(&args[0])?; self.emit_unpack_string();
            self.wfunc.instruction(&W::Call(self.cg.alert_idx));
            return Ok(());
        }
        if func == "canvas_fill_rect" && args.len() == 6 {
            self.emit_operand(&args[0])?; self.emit_unpack_string();  // id off,len
            self.emit_operand(&args[1])?;                              // x i64
            self.emit_operand(&args[2])?;                              // y i64
            self.emit_operand(&args[3])?;                              // w i64
            self.emit_operand(&args[4])?;                              // h i64
            self.emit_operand(&args[5])?; self.emit_unpack_string();   // color off,len
            self.wfunc.instruction(&W::Call(self.cg.canvas_fill_rect_idx));
            return Ok(());
        }
        if func == "canvas_clear" && args.len() == 1 {
            self.emit_operand(&args[0])?; self.emit_unpack_string();
            self.wfunc.instruction(&W::Call(self.cg.canvas_clear_idx));
            return Ok(());
        }
        if func == "fetch_text" && args.len() == 1 {
            self.emit_operand(&args[0])?; self.emit_unpack_string();
            self.wfunc.instruction(&W::Call(self.cg.fetch_text_idx));
            return Ok(());
        }

        // -------------------------------------------------------------
        // Built-in: to_string(v) — convert i64/bool/f64 to decimal string
        // -------------------------------------------------------------
        if func == "to_string" && args.len() == 1 {
            let ty = self.operand_ty(&args[0]).unwrap_or(MirType::I64);
            self.emit_operand(&args[0])?;
            match ty {
                MirType::F64 => {
                    self.wfunc.instruction(&W::Call(self.cg.to_string_f64_idx));
                }
                MirType::F32 => {
                    self.wfunc.instruction(&W::F64PromoteF32);
                    self.wfunc.instruction(&W::Call(self.cg.to_string_f64_idx));
                }
                _ => {
                    self.wfunc.instruction(&W::Call(self.cg.to_string_i64_idx));
                }
            }
            return Ok(());
        }

        // -------------------------------------------------------------
        // Built-in: parse_int(s) / parse_float(s) — string -> number
        // -------------------------------------------------------------
        if func == "parse_int" && args.len() == 1 {
            self.emit_operand(&args[0])?;
            self.wfunc.instruction(&W::Call(self.cg.string_parse_int_idx));
            return Ok(());
        }
        if func == "parse_float" && args.len() == 1 {
            self.emit_operand(&args[0])?;
            self.wfunc.instruction(&W::Call(self.cg.string_parse_float_idx));
            return Ok(());
        }

        // -------------------------------------------------------------
        // Built-in: push(arr, value) — append value to array, return new handle
        // -------------------------------------------------------------
        if func == "push" && args.len() == 2 {
            self.emit_operand(&args[0])?; // packed array i64
            self.emit_operand(&args[1])?; // value i64
            self.wfunc.instruction(&W::Call(self.cg.array_push_idx));
            return Ok(());
        }

        // -------------------------------------------------------------
        // User function call
        // -------------------------------------------------------------
        if let Some(&idx) = self.cg.fn_indices.get(func) {
            for a in args {
                self.emit_operand(a)?;
            }
            self.wfunc.instruction(&W::Call(idx));
            Ok(())
        } else {
            Err(WasmCodegenError::unsupported(&format!(
                "call to `{func}` — supported builtins: println, len, str_concat, array_new/get/set, to_string"
            )))
        }
    }

    /// Emit f64 binary operations.
    fn emit_binop_f64(&mut self, op: MirBinOp) -> Result<(), WasmCodegenError> {
        match op {
            MirBinOp::Add => { self.wfunc.instruction(&W::F64Add); }
            MirBinOp::Sub => { self.wfunc.instruction(&W::F64Sub); }
            MirBinOp::Mul => { self.wfunc.instruction(&W::F64Mul); }
            MirBinOp::Div => { self.wfunc.instruction(&W::F64Div); }
            MirBinOp::Eq  => { self.wfunc.instruction(&W::F64Eq);  self.wfunc.instruction(&W::I64ExtendI32U); }
            MirBinOp::Neq => { self.wfunc.instruction(&W::F64Ne);  self.wfunc.instruction(&W::I64ExtendI32U); }
            MirBinOp::Lt  => { self.wfunc.instruction(&W::F64Lt);  self.wfunc.instruction(&W::I64ExtendI32U); }
            MirBinOp::Gt  => { self.wfunc.instruction(&W::F64Gt);  self.wfunc.instruction(&W::I64ExtendI32U); }
            MirBinOp::LtEq => { self.wfunc.instruction(&W::F64Le); self.wfunc.instruction(&W::I64ExtendI32U); }
            MirBinOp::GtEq => { self.wfunc.instruction(&W::F64Ge); self.wfunc.instruction(&W::I64ExtendI32U); }
            other => {
                return Err(WasmCodegenError::unsupported(&format!(
                    "f64 binary op `{other:?}`"
                )));
            }
        }
        Ok(())
    }

    /// Emit an operand as a packed str i64 (calling to_string if the type is not Str).
    fn emit_to_str_packed(&mut self, op: &Operand) -> Result<(), WasmCodegenError> {
        let ty = self.operand_ty(op).unwrap_or(MirType::I64);
        self.emit_operand(op)?;
        match ty {
            MirType::Str => { /* already packed str */ }
            MirType::F64 => {
                self.wfunc.instruction(&W::Call(self.cg.to_string_f64_idx));
            }
            MirType::F32 => {
                self.wfunc.instruction(&W::F64PromoteF32);
                self.wfunc.instruction(&W::Call(self.cg.to_string_f64_idx));
            }
            _ => {
                self.wfunc.instruction(&W::Call(self.cg.to_string_i64_idx));
            }
        }
        Ok(())
    }

    /// Emit a StringConcat rvalue (string interpolation: N parts → pairwise concat).
    fn emit_string_concat(&mut self, parts: &[Operand]) -> Result<(), WasmCodegenError> {
        if parts.is_empty() {
            let (off, len) = self.cg.intern_string("");
            let packed = ((len as u64) << 32) | (off as u64);
            self.wfunc.instruction(&W::I64Const(packed as i64));
            return Ok(());
        }
        if parts.len() == 1 {
            return self.emit_to_str_packed(&parts[0]);
        }
        // Pairwise left-fold: acc = concat(p0, p1); for p in p2..: acc = concat(acc, p)
        self.emit_to_str_packed(&parts[0])?;
        self.emit_unpack_string();
        self.emit_to_str_packed(&parts[1])?;
        self.emit_unpack_string();
        self.wfunc.instruction(&W::Call(self.cg.string_concat_idx));
        for part in &parts[2..] {
            // Stack: [packed_acc]
            self.emit_unpack_string();              // [off_acc, len_acc]
            self.emit_to_str_packed(part)?;         // [off_acc, len_acc, packed_next]
            self.emit_unpack_string();              // [off_acc, len_acc, off_next, len_next]
            self.wfunc.instruction(&W::Call(self.cg.string_concat_idx));
        }
        Ok(())
    }

    /// Emit a Switch terminator as a flat chain of `if` blocks.
    fn emit_switch(
        &mut self,
        value: Operand,
        targets: Vec<(i64, BlockId)>,
        default: BlockId,
    ) -> Result<(), WasmCodegenError> {
        // Collect unique arm blocks in order of first appearance, grouping
        // values that map to the same block (or-patterns: 1|2|3 => ...).
        let mut seen_ids: Vec<u32> = Vec::new();
        let mut block_vals: HashMap<u32, (BlockId, Vec<i64>)> = HashMap::new();
        for (val, bid) in &targets {
            if let Some(entry) = block_vals.get_mut(&bid.0) {
                entry.1.push(*val);
            } else {
                seen_ids.push(bid.0);
                block_vals.insert(bid.0, (*bid, vec![*val]));
            }
        }

        // Detect whether any arm Goto's a join block (vs returning directly).
        let join = self.find_switch_join(&targets, default);

        // Wrap in a block so Goto(join) arms can break out with `br 1`.
        if join.is_some() {
            self.wfunc.instruction(&W::Block(BlockType::Empty));
        }

        // Emit one `if` block per unique arm.
        for &bid_u32 in &seen_ids {
            let (bid, vals) = &block_vals[&bid_u32];
            self.emit_switch_cond(&value, vals)?;
            self.wfunc.instruction(&W::If(BlockType::Empty));
            let arm_idx = self.block_index(*bid)?;
            if !self.visited[arm_idx] {
                self.visited[arm_idx] = true;
                let instrs: Vec<_> = self.func.blocks[arm_idx].instructions.clone();
                for inst in &instrs {
                    self.emit_instruction(inst)?;
                }
                let term = self.func.blocks[arm_idx].terminator.clone();
                self.emit_switch_arm_term(&term, join)?;
            }
            self.wfunc.instruction(&W::End);
        }

        // Emit the default arm (always reached if no earlier arm matched).
        let default_idx = self.block_index(default)?;
        if !self.visited[default_idx] {
            self.visited[default_idx] = true;
            let instrs: Vec<_> = self.func.blocks[default_idx].instructions.clone();
            for inst in &instrs {
                self.emit_instruction(inst)?;
            }
            let term = self.func.blocks[default_idx].terminator.clone();
            // Default arm is at depth 0 inside the wrapper block (no nested if),
            // so Goto(join) just falls through to the wrapper `end`.
            match term {
                Terminator::Return(ref v) => {
                    if let Some(op) = v.as_ref() {
                        self.emit_operand(op)?;
                    }
                    self.wfunc.instruction(&W::Return);
                }
                Terminator::Goto(j) if Some(j) == join => { /* fall through */ }
                _ => { self.wfunc.instruction(&W::Unreachable); }
            }
        }

        if join.is_some() {
            self.wfunc.instruction(&W::End); // end wrapper block
        }

        if let Some(j) = join {
            self.emit_block(j)?;
        }
        Ok(())
    }

    /// Emit the terminator for a non-default switch arm (inside an `if` block).
    fn emit_switch_arm_term(&mut self, term: &Terminator, join: Option<BlockId>) -> Result<(), WasmCodegenError> {
        match term {
            Terminator::Return(ref v) => {
                if let Some(op) = v.as_ref() {
                    self.emit_operand(op)?;
                }
                self.wfunc.instruction(&W::Return);
            }
            Terminator::Goto(j) if Some(*j) == join => {
                // Break out of: if (depth 0) + wrapper block (depth 1).
                self.wfunc.instruction(&W::Br(1));
            }
            _ => { self.wfunc.instruction(&W::Unreachable); }
        }
        Ok(())
    }

    /// Emit the condition for a switch arm: value == vals[0] || value == vals[1] || ...
    /// Leaves an i32 (0/1) on the stack (WASM comparison ops return i32).
    fn emit_switch_cond(&mut self, value: &Operand, vals: &[i64]) -> Result<(), WasmCodegenError> {
        self.emit_operand(value)?;
        self.wfunc.instruction(&W::I64Const(vals[0]));
        self.wfunc.instruction(&W::I64Eq);
        for &v in &vals[1..] {
            self.emit_operand(value)?;
            self.wfunc.instruction(&W::I64Const(v));
            self.wfunc.instruction(&W::I64Eq);
            self.wfunc.instruction(&W::I32Or);
        }
        Ok(())
    }

    /// Return the join block for a switch: the block that all arms Goto, if any.
    fn find_switch_join(&self, targets: &[(i64, BlockId)], default: BlockId) -> Option<BlockId> {
        let default_idx = *self.id_to_index.get(&default.0)?;
        if let Terminator::Goto(j) = self.func.blocks[default_idx].terminator {
            return Some(j);
        }
        for (_, bid) in targets {
            let idx = *self.id_to_index.get(&bid.0)?;
            if let Terminator::Goto(j) = self.func.blocks[idx].terminator {
                return Some(j);
            }
        }
        None
    }

    fn emit_cast(&mut self, ty: &MirType) -> Result<(), WasmCodegenError> {
        match ty {
            MirType::I64 | MirType::U64 | MirType::Bool => {
                // No-op (already i64).
            }
            MirType::F64 => {
                self.wfunc.instruction(&W::F64ConvertI64S);
            }
            MirType::I32 | MirType::U32 => {
                self.wfunc.instruction(&W::I32WrapI64);
                self.wfunc.instruction(&W::I64ExtendI32S);
            }
            _ => {
                return Err(WasmCodegenError::unsupported(&format!(
                    "cast to type `{ty}`"
                )));
            }
        }
        Ok(())
    }
}

/// Find the first block both arms reach (a shared successor). Returns None
/// if both arms terminate without reconverging.
fn find_common_join(
    func: &MirFunction,
    id_to_index: &HashMap<u32, usize>,
    then_block: BlockId,
    else_block: BlockId,
) -> Option<BlockId> {
    use std::collections::HashSet;
    fn walk(
        func: &MirFunction,
        id_to_index: &HashMap<u32, usize>,
        start: BlockId,
        seen: &mut HashSet<u32>,
    ) {
        let mut stack = vec![start];
        while let Some(b) = stack.pop() {
            if !seen.insert(b.0) {
                continue;
            }
            let idx = match id_to_index.get(&b.0) {
                Some(&i) => i,
                None => continue,
            };
            let block = &func.blocks[idx];
            match &block.terminator {
                Terminator::Goto(t) => stack.push(*t),
                Terminator::Branch {
                    then_block,
                    else_block,
                    ..
                } => {
                    stack.push(*then_block);
                    stack.push(*else_block);
                }
                _ => {}
            }
        }
    }
    let mut then_set = HashSet::new();
    walk(func, id_to_index, then_block, &mut then_set);
    let mut else_set = HashSet::new();
    walk(func, id_to_index, else_block, &mut else_set);
    // Lowest-id common block.
    let mut common: Vec<u32> = then_set.intersection(&else_set).copied().collect();
    common.sort_unstable();
    common.first().map(|&id| BlockId(id))
}

fn debug_short(rv: &RValue) -> String {
    // One-word tag for error messages.
    match rv {
        RValue::Use(_) => "use".into(),
        RValue::BinOp { .. } => "binop".into(),
        RValue::UnOp { .. } => "unop".into(),
        RValue::Call { .. } => "call".into(),
        RValue::CallIndirect { .. } => "call-indirect".into(),
        RValue::ConstInt(_) => "const-int".into(),
        RValue::ConstFloat(_) => "const-float".into(),
        RValue::ConstBool(_) => "const-bool".into(),
        RValue::ConstString(_) => "const-string".into(),
        RValue::ConstNone => "const-none".into(),
        RValue::Array(_) => "array".into(),
        RValue::Tuple(_) => "tuple".into(),
        RValue::Struct { .. } => "struct".into(),
        RValue::Field { .. } => "field".into(),
        RValue::Index { .. } => "index".into(),
        RValue::ArcAlloc { .. } => "arc-alloc".into(),
        RValue::Cast { .. } => "cast".into(),
        RValue::EnumVariant { .. } => "enum-variant".into(),
        RValue::EnumTag { .. } => "enum-tag".into(),
        RValue::EnumPayload { .. } => "enum-payload".into(),
        RValue::Closure { .. } => "closure".into(),
        RValue::Map(_) => "map".into(),
        RValue::StringConcat(_) => "string-concat".into(),
        RValue::Range { .. } => "range".into(),
        RValue::AddrOf { .. } => "addr-of".into(),
        RValue::Deref { .. } => "deref".into(),
        RValue::Comptime(_) => "comptime".into(),
        RValue::MakeTraitObject { .. } => "trait-object".into(),
        RValue::VtableCall { .. } => "vtable-call".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn empty_module_emits() {
        let module = MirModule {
            functions: vec![],
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            trait_vtables: HashMap::new(),
            copy_structs: HashSet::new(),
        };
        let bytes = emit_module(&module, &WasmOptions::default()).expect("emit");
        // WASM magic + version
        assert_eq!(&bytes[0..4], b"\0asm");
    }
}
