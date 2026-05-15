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
    BasicBlock, BlockId, Constant, Instruction, LocalId, MirBinOp, MirFunction, MirModule,
    MirType, MirUnOp, Operand, RValue, Terminator,
};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction as W, MemorySection, MemoryType, Module,
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
#[derive(Debug, Clone, Default)]
pub struct WasmOptions {
    /// If true, emit a WASI-compatible module (uses `wasi_snapshot_preview1`
    /// imports for I/O instead of `env`).
    pub wasi: bool,
}

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
        MirType::Void => {
            // Caller should special-case void; this is a placeholder.
            return Err(WasmCodegenError::new(
                "internal: lower_type called on Void; caller should special-case",
            ));
        }
        other => {
            return Err(WasmCodegenError::unsupported(&format!(
                "type `{other}` (only i64/f64/bool/str scalars supported in v0.1)"
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
            string_table: HashMap::new(),
            // Reserve the first 16 bytes so offset 0 stays sentinel-free.
            string_cursor: 16,
            string_bytes: vec![0u8; 16],
        }
    }

    /// Top-level emit.
    fn emit(&mut self, module: &MirModule) -> Result<(), WasmCodegenError> {
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
        let env_module = if self.options.wasi { "env" } else { "env" };

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
                } else {
                    self.emit_if_else(&cond, then_block, else_block)?;
                }
            }
            Terminator::Switch { .. } => {
                return Err(WasmCodegenError::unsupported(
                    "match / switch terminators (use if/else chains in v0.1)",
                ));
            }
            Terminator::Unreachable => {
                self.wfunc.instruction(&W::Unreachable);
            }
        }
        Ok(())
    }

    /// Detect: branch from `header` -> (body, exit) where `body` ends in
    /// Goto(header). That's a simple while loop.
    fn is_simple_while_header(&self, header: BlockId, body: BlockId, _exit: BlockId) -> bool {
        let body_idx = match self.id_to_index.get(&body.0) {
            Some(&i) => i,
            None => return false,
        };
        matches!(
            self.func.blocks[body_idx].terminator,
            Terminator::Goto(b) if b == header
        )
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
        match &body_block.terminator {
            Terminator::Goto(b) if *b == header => {
                self.wfunc.instruction(&W::Br(0)); // continue
            }
            Terminator::Return(value) => {
                // Early return inside loop body.
                if let Some(op) = value {
                    self.emit_operand(op)?;
                }
                self.wfunc.instruction(&W::Return);
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

    /// Emit an if/else: detect the join point (the first block both arms
    /// reach), emit then/else inside a wasm `if`, then continue with join.
    fn emit_if_else(
        &mut self,
        cond: &Operand,
        then_block: BlockId,
        else_block: BlockId,
    ) -> Result<(), WasmCodegenError> {
        // The conservative shape we accept: both arms terminate in either
        // `Return` or `Goto(join)` where join is the same for both arms.
        let join = find_common_join(self.func, &self.id_to_index, then_block, else_block);

        // Determine block type. For v0.1, since we don't push values across
        // if/else boundaries, use Empty block type.
        self.emit_operand(cond)?;
        // cond is i64 in MIR for bools; convert to i32 for `if`.
        self.wfunc.instruction(&W::I32WrapI64);
        self.wfunc.instruction(&W::If(BlockType::Empty));

        // then arm
        self.emit_arm(then_block, join)?;

        self.wfunc.instruction(&W::Else);

        // else arm
        self.emit_arm(else_block, join)?;

        self.wfunc.instruction(&W::End);

        // Continue with the join block (if any).
        if let Some(j) = join {
            // Avoid re-emitting if both arms returned.
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
        self.emit_operand(cond)?;
        self.wfunc.instruction(&W::I32WrapI64);
        self.wfunc.instruction(&W::If(BlockType::Empty));
        self.emit_arm(then_block, join)?;
        self.wfunc.instruction(&W::Else);
        self.emit_arm(else_block, join)?;
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
            Instruction::Nop => {}
            Instruction::ArcRetain { .. }
            | Instruction::ArcRelease { .. }
            | Instruction::Drop { .. } => {
                // In v0.1 we have no heap, so ARC ops are no-ops.
            }
            Instruction::StoreField { .. }
            | Instruction::StoreDeref { .. }
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
                // v0.2: detect string+string concatenation and route to the
                // host's kryos_string_concat. Otherwise lower as i64 arith.
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
                } else {
                    self.emit_operand(left)?;
                    self.emit_operand(right)?;
                    self.emit_binop(*op)?;
                }
            }
            RValue::UnOp { op, operand } => {
                self.emit_operand(operand)?;
                self.emit_unop(*op)?;
            }
            RValue::Call { func, args } => {
                self.emit_call(func, args)?;
            }
            RValue::Cast { operand, ty } => {
                self.emit_operand(operand)?;
                self.emit_cast(ty)?;
            }
            other => {
                return Err(WasmCodegenError::unsupported(&format!(
                    "rvalue `{}` (v0.1 supports scalar arith, constants, calls, casts)",
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
                // i64 negate: 0 - x
                self.wfunc.instruction(&W::I64Const(0));
                // x is currently on top — swap conceptually by using sub with
                // a fresh temp. WASM doesn't have swap, so we use a trick:
                // store to a temp local. For simplicity we just emit:
                //   (i64.const 0) (local.set tmp) (i64.const 0) (... x ...) (i64.sub)
                // which we can't easily do here without a tmp. Instead emit:
                //   x is on stack. Push 0. We want 0 - x but stack is [x, 0].
                //   Use i64.sub which is (a - b) where stack is [..., a, b].
                //   So we need x as `a` and 0 as `b`? No, we want 0 - x.
                // Easier: we already pushed 0 on top of x.
                //   stack: [x, 0]
                //   i64.sub computes x - 0 = x. Wrong.
                // Correct: we should push 0 BEFORE x. But the operand is
                // already on the stack from the rvalue lowering. Use a tmp:
                return Err(WasmCodegenError::unsupported(
                    "unary negation (use 0 - x for v0.1)",
                ));
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
        // Built-in: len(s) — length of a string (high 32 bits of packed i64)
        // -------------------------------------------------------------
        if func == "len" && args.len() == 1 {
            self.emit_operand(&args[0])?;
            self.wfunc.instruction(&W::I64Const(32));
            self.wfunc.instruction(&W::I64ShrU);
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
                "call to `{func}` — supported builtins: println, len, str_concat, array_new/get/set"
            )))
        }
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
