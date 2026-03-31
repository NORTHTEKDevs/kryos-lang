"""
Kryos Language Server -- LSP implementation over JSON-RPC / stdin-stdout.

Provides diagnostics, hover, completion, go-to-definition, and document
symbols for .kry files.  Pure Python stdlib -- no pip dependencies.

Launch via:  kryos lsp
             python -m kryos.lsp.server
"""

from __future__ import annotations

import json
import logging
import os
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import unquote, urlparse
from urllib.request import pathname2url

from kryos.lsp.protocol import (
    read_message,
    write_message,
    make_response,
    make_error,
    make_notification,
    METHOD_NOT_FOUND,
    INTERNAL_ERROR,
    Position,
    Range,
    Location,
    Diagnostic,
    DiagnosticSeverity,
    CompletionItem,
    CompletionItemKind,
    DocumentSymbol,
    SymbolKind,
    Hover,
    TextDocumentSyncKind,
)

# Compiler imports
from kryos.compiler.lexer import Lexer, LexerError, tokenize
from kryos.compiler.parser import parse, ParseError, ParseErrors
from kryos.compiler.tokens import TokenType, KEYWORDS, ATTRIBUTE_KEYWORDS, BUILTIN_TYPES
from kryos.compiler.ast_nodes import (
    Module,
    FnDecl,
    StructDecl,
    EnumDecl,
    TraitDecl,
    ImplBlock,
    ActorDecl,
    TypeAlias,
    LetStmt,
    ForStmt,
    Parameter,
    Identifier,
    FieldAccess,
    Attribute,
)

# ---------------------------------------------------------------------------
# Logging  (writes to stderr so it never contaminates the JSON-RPC channel)
# ---------------------------------------------------------------------------

logging.basicConfig(
    stream=sys.stderr,
    level=logging.WARNING,
    format="[kryos-lsp] %(levelname)s: %(message)s",
)
log = logging.getLogger("kryos-lsp")


# ---------------------------------------------------------------------------
# URI helpers
# ---------------------------------------------------------------------------

def uri_to_path(uri: str) -> str:
    """Convert a file:// URI to a local filesystem path."""
    parsed = urlparse(uri)
    path = unquote(parsed.path)
    # On Windows, paths come as /C:/foo -- strip the leading slash
    if len(path) > 2 and path[0] == "/" and path[2] == ":":
        path = path[1:]
    return path


def path_to_uri(path: str) -> str:
    """Convert a local filesystem path to a file:// URI."""
    p = Path(path).resolve().as_posix()
    if not p.startswith("/"):
        p = "/" + p
    return "file://" + p


# ---------------------------------------------------------------------------
# Built-in knowledge base (for hover & completion)
# ---------------------------------------------------------------------------

BUILTIN_FUNCTIONS: Dict[str, Tuple[str, str]] = {
    "println":  ("fn println(args: ...any)",     "Print arguments followed by a newline."),
    "print":    ("fn print(args: ...any)",       "Print arguments without a trailing newline."),
    "len":      ("fn len(collection) -> i64",    "Return the length of a collection or string."),
    "range":    ("fn range(start, end, step?)",  "Generate an integer range."),
    "push":     ("fn push(vec, item)",           "Append an item to a vector."),
    "pop":      ("fn pop(vec) -> item",          "Remove and return the last item."),
    "abs":      ("fn abs(x) -> number",          "Return the absolute value."),
    "min":      ("fn min(a, b) -> number",       "Return the smaller of two values."),
    "max":      ("fn max(a, b) -> number",       "Return the larger of two values."),
    "sqrt":     ("fn sqrt(x) -> f64",            "Return the square root of x."),
    "typeof":   ("fn typeof(x) -> str",          "Return the runtime type name."),
    "input":    ("fn input(prompt?: str) -> str", "Read a line from stdin."),
    "assert":   ("fn assert(cond: bool, msg?)",  "Assert a condition; panic if false."),
    "panic":    ("fn panic(msg: str)",           "Terminate with an error message."),
    "to_string": ("fn to_string(x) -> str",     "Convert a value to its string representation."),
    "to_int":   ("fn to_int(x) -> i64",          "Parse a string or cast to integer."),
    "to_float": ("fn to_float(x) -> f64",        "Parse a string or cast to float."),
}

KEYWORD_DESCRIPTIONS: Dict[str, str] = {
    "fn":       "Declare a function.",
    "let":      "Declare a variable binding.",
    "mut":      "Mark a binding as mutable.",
    "return":   "Return a value from the current function.",
    "if":       "Conditional branch.",
    "else":     "Alternate branch of an `if`.",
    "elif":     "Chained conditional branch.",
    "for":      "Iterate over a range or collection.",
    "while":    "Loop while a condition is true.",
    "in":       "Used in `for ... in` loops.",
    "break":    "Exit the innermost loop.",
    "continue": "Skip to the next loop iteration.",
    "struct":   "Define a product type (record).",
    "enum":     "Define a sum type (tagged union).",
    "impl":     "Implement methods or a trait for a type.",
    "trait":    "Define a trait (interface).",
    "pub":      "Mark an item as publicly visible.",
    "use":      "Import a module or symbol.",
    "extern":   "Declare an external (FFI) binding.",
    "as":       "Type cast or import alias.",
    "mod":      "Declare a sub-module.",
    "type":     "Declare a type alias.",
    "true":     "Boolean literal `true`.",
    "false":    "Boolean literal `false`.",
    "none":     "The absence-of-value literal.",
    "actor":    "Declare an actor (concurrent entity).",
    "spawn":    "Spawn an actor or parallel task.",
    "parallel": "Execute a loop body in parallel.",
    "quantum":  "Enter a quantum computing block.",
    "comptime": "Execute a block at compile time.",
    "and":      "Logical AND operator.",
    "or":       "Logical OR operator.",
    "not":      "Logical NOT operator.",
}

BUILTIN_TYPE_DESCRIPTIONS: Dict[str, str] = {
    "i8":    "8-bit signed integer",
    "i16":   "16-bit signed integer",
    "i32":   "32-bit signed integer",
    "i64":   "64-bit signed integer",
    "i128":  "128-bit signed integer",
    "u8":    "8-bit unsigned integer",
    "u16":   "16-bit unsigned integer",
    "u32":   "32-bit unsigned integer",
    "u64":   "64-bit unsigned integer",
    "u128":  "128-bit unsigned integer",
    "f32":   "32-bit floating point",
    "f64":   "64-bit floating point",
    "bool":  "Boolean (`true` / `false`)",
    "str":   "UTF-8 string",
    "char":  "Unicode scalar value",
    "Tensor": "Multi-dimensional numeric array (GPU-accelerable)",
    "Vec":   "Growable array / vector",
    "Map":   "Hash map (key-value store)",
    "Set":   "Hash set (unique values)",
    "Option": "Optional value (`Some(T)` or `None`)",
    "Result": "Result type (`Ok(T)` or `Err(E)`)",
    "Secret": "Capability-gated secret value",
    "Qubit":  "Single quantum bit",
    "Qureg":  "Quantum register (array of qubits)",
}

ATTRIBUTE_DESCRIPTIONS: Dict[str, str] = {
    "capabilities":   "Declare required capabilities (e.g. `@capabilities(\"gpu\", \"network\")`).",
    "compute":        "Specify compute target (e.g. `@compute(device=\"cuda\")`).",
    "export":         "Mark a function for public API export.",
    "layout":         "Specify memory layout for a struct.",
    "real_time":      "Mark a function as real-time safe (no allocations).",
    "no_std":         "Compile without the standard library.",
    "zero_copy":      "Enable zero-copy semantics for a parameter.",
    "target":         "Restrict compilation to a specific target.",
    "differentiable": "Enable automatic differentiation.",
}


# ---------------------------------------------------------------------------
# Document state
# ---------------------------------------------------------------------------

@dataclass
class DocumentState:
    """Cached state for an open document."""
    uri: str
    version: int
    text: str
    module: Optional[Module] = None
    diagnostics: List[Diagnostic] = None  # type: ignore[assignment]

    # Indexes built from a successful parse
    fn_defs: Dict[str, Tuple[FnDecl, str]] = None  # type: ignore[assignment]
    struct_defs: Dict[str, Tuple[StructDecl, str]] = None  # type: ignore[assignment]
    enum_defs: Dict[str, Tuple[EnumDecl, str]] = None  # type: ignore[assignment]

    def __post_init__(self) -> None:
        if self.diagnostics is None:
            self.diagnostics = []
        if self.fn_defs is None:
            self.fn_defs = {}
        if self.struct_defs is None:
            self.struct_defs = {}
        if self.enum_defs is None:
            self.enum_defs = {}


# ---------------------------------------------------------------------------
# Server
# ---------------------------------------------------------------------------

class KryosLanguageServer:
    """The main LSP server loop."""

    def __init__(self) -> None:
        self._running = True
        self._initialized = False
        self._documents: Dict[str, DocumentState] = {}
        self._root_uri: Optional[str] = None

    # ------------------------------------------------------------------
    # Main loop
    # ------------------------------------------------------------------

    def run(self) -> None:
        """Block on stdin, dispatching JSON-RPC messages until exit."""
        log.info("Kryos language server starting")
        while self._running:
            msg = read_message()
            if msg is None:
                break
            self._dispatch(msg)
        log.info("Kryos language server exiting")

    # ------------------------------------------------------------------
    # Dispatch
    # ------------------------------------------------------------------

    def _dispatch(self, msg: Dict[str, Any]) -> None:
        method = msg.get("method", "")
        req_id = msg.get("id")  # None for notifications
        params = msg.get("params", {})

        handler = {
            "initialize":                       self._on_initialize,
            "initialized":                      self._on_initialized,
            "shutdown":                         self._on_shutdown,
            "exit":                             self._on_exit,
            "textDocument/didOpen":             self._on_did_open,
            "textDocument/didChange":           self._on_did_change,
            "textDocument/didSave":             self._on_did_save,
            "textDocument/didClose":            self._on_did_close,
            "textDocument/hover":               self._on_hover,
            "textDocument/completion":          self._on_completion,
            "textDocument/definition":          self._on_definition,
            "textDocument/documentSymbol":      self._on_document_symbol,
        }.get(method)

        if handler is None:
            # Ignore unknown notifications; respond to unknown requests
            if req_id is not None:
                write_message(make_error(req_id, METHOD_NOT_FOUND,
                                         f"Method not found: {method}"))
            return

        try:
            result = handler(req_id, params)
            if req_id is not None and result is not None:
                write_message(make_response(req_id, result))
        except Exception as exc:
            log.exception("Handler error for %s", method)
            if req_id is not None:
                write_message(make_error(req_id, INTERNAL_ERROR, str(exc)))

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def _on_initialize(self, req_id: Any, params: Dict[str, Any]) -> Any:
        self._root_uri = params.get("rootUri")
        return {
            "capabilities": {
                "textDocumentSync": {
                    "openClose": True,
                    "change": TextDocumentSyncKind.Full,
                    "save": {"includeText": True},
                },
                "hoverProvider": True,
                "completionProvider": {
                    "triggerCharacters": [".", "@"],
                    "resolveProvider": False,
                },
                "definitionProvider": True,
                "documentSymbolProvider": True,
            },
            "serverInfo": {
                "name": "kryos-lsp",
                "version": "0.1.0",
            },
        }

    def _on_initialized(self, _req_id: Any, _params: Dict[str, Any]) -> None:
        self._initialized = True
        log.info("Client initialized")

    def _on_shutdown(self, req_id: Any, _params: Dict[str, Any]) -> Any:
        self._initialized = False
        return None  # success (null result)

    def _on_exit(self, _req_id: Any, _params: Dict[str, Any]) -> None:
        self._running = False

    # ------------------------------------------------------------------
    # Document sync
    # ------------------------------------------------------------------

    def _on_did_open(self, _req_id: Any, params: Dict[str, Any]) -> None:
        td = params["textDocument"]
        uri = td["uri"]
        self._documents[uri] = DocumentState(
            uri=uri,
            version=td.get("version", 0),
            text=td["text"],
        )
        self._validate_document(uri)

    def _on_did_change(self, _req_id: Any, params: Dict[str, Any]) -> None:
        uri = params["textDocument"]["uri"]
        doc = self._documents.get(uri)
        if doc is None:
            return
        for change in params.get("contentChanges", []):
            # Full sync -- each change replaces the entire text
            doc.text = change["text"]
        doc.version = params["textDocument"].get("version", doc.version + 1)
        self._validate_document(uri)

    def _on_did_save(self, _req_id: Any, params: Dict[str, Any]) -> None:
        uri = params["textDocument"]["uri"]
        doc = self._documents.get(uri)
        if doc is None:
            return
        if "text" in params:
            doc.text = params["text"]
        self._validate_document(uri)

    def _on_did_close(self, _req_id: Any, params: Dict[str, Any]) -> None:
        uri = params["textDocument"]["uri"]
        self._documents.pop(uri, None)
        # Clear diagnostics
        write_message(make_notification("textDocument/publishDiagnostics", {
            "uri": uri,
            "diagnostics": [],
        }))

    # ------------------------------------------------------------------
    # Validation / diagnostics
    # ------------------------------------------------------------------

    def _validate_document(self, uri: str) -> None:
        doc = self._documents.get(uri)
        if doc is None:
            return

        diagnostics: List[Diagnostic] = []
        filepath = uri_to_path(uri)

        # --- Tokenize ---
        try:
            tokens = tokenize(doc.text, filepath)
        except LexerError as e:
            line = max(e.line - 1, 0)  # LSP is 0-based
            col = max(e.column - 1, 0)
            diagnostics.append(Diagnostic(
                range=Range(Position(line, col), Position(line, col + 1)),
                severity=DiagnosticSeverity.Error,
                message=str(e),
            ))
            doc.diagnostics = diagnostics
            doc.module = None
            self._publish_diagnostics(uri, diagnostics)
            return

        # Report any ERROR tokens from the lexer
        for tok in tokens:
            if tok.type == TokenType.ERROR:
                line = max(tok.line - 1, 0)
                col = max(tok.column - 1, 0)
                diagnostics.append(Diagnostic(
                    range=Range(Position(line, col), Position(line, col + len(tok.value))),
                    severity=DiagnosticSeverity.Error,
                    message=f"Lexer error: {tok.value}",
                ))

        # --- Parse ---
        try:
            module = parse(tokens)
        except ParseError as e:
            line = max(e.line - 1, 0)
            col = max(e.column - 1, 0)
            diagnostics.append(Diagnostic(
                range=Range(Position(line, col), Position(line, col + 1)),
                severity=DiagnosticSeverity.Error,
                message=e.message,
            ))
            doc.diagnostics = diagnostics
            doc.module = None
            self._publish_diagnostics(uri, diagnostics)
            return
        except ParseErrors as e:
            for pe in e.errors:
                line = max(pe.line - 1, 0)
                col = max(pe.column - 1, 0)
                diagnostics.append(Diagnostic(
                    range=Range(Position(line, col), Position(line, col + 1)),
                    severity=DiagnosticSeverity.Error,
                    message=pe.message,
                ))
            doc.diagnostics = diagnostics
            doc.module = None
            self._publish_diagnostics(uri, diagnostics)
            return

        doc.module = module

        # --- Build definition indexes ---
        self._build_indexes(doc)

        # --- Capability analysis (best-effort) ---
        try:
            from kryos.compiler.capabilities import CapabilityAnalyzer, CapabilityTier
            analyzer = CapabilityAnalyzer(tier=CapabilityTier.COMMUNITY)
            report = analyzer.analyze(module)
            for err_msg in report.get("errors", []):
                diagnostics.append(Diagnostic(
                    range=Range(Position(0, 0), Position(0, 1)),
                    severity=DiagnosticSeverity.Warning,
                    message=f"Capability: {err_msg}",
                ))
            for warn_msg in report.get("warnings", []):
                diagnostics.append(Diagnostic(
                    range=Range(Position(0, 0), Position(0, 1)),
                    severity=DiagnosticSeverity.Information,
                    message=f"Capability: {warn_msg}",
                ))
        except Exception:
            pass  # capability analysis is best-effort

        doc.diagnostics = diagnostics
        self._publish_diagnostics(uri, diagnostics)

    def _publish_diagnostics(self, uri: str, diagnostics: List[Diagnostic]) -> None:
        write_message(make_notification("textDocument/publishDiagnostics", {
            "uri": uri,
            "diagnostics": [d.to_dict() for d in diagnostics],
        }))

    def _build_indexes(self, doc: DocumentState) -> None:
        """Index functions, structs, enums from a parsed module."""
        doc.fn_defs.clear()
        doc.struct_defs.clear()
        doc.enum_defs.clear()

        if doc.module is None:
            return

        for decl in doc.module.declarations:
            if isinstance(decl, FnDecl):
                doc.fn_defs[decl.name] = (decl, doc.uri)
            elif isinstance(decl, StructDecl):
                doc.struct_defs[decl.name] = (decl, doc.uri)
            elif isinstance(decl, EnumDecl):
                doc.enum_defs[decl.name] = (decl, doc.uri)
            elif isinstance(decl, ImplBlock):
                for m in decl.methods:
                    doc.fn_defs[m.name] = (m, doc.uri)
            elif isinstance(decl, TraitDecl):
                for m in decl.methods:
                    doc.fn_defs[m.name] = (m, doc.uri)
            elif isinstance(decl, ActorDecl):
                for h in decl.handlers:
                    doc.fn_defs[h.name] = (h, doc.uri)

    # ------------------------------------------------------------------
    # Hover
    # ------------------------------------------------------------------

    def _on_hover(self, req_id: Any, params: Dict[str, Any]) -> Any:
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        doc = self._documents.get(uri)
        if doc is None:
            return None

        line_num = pos["line"]
        char_num = pos["character"]

        word = self._word_at(doc.text, line_num, char_num)
        if not word:
            return None

        # Check built-in functions
        if word in BUILTIN_FUNCTIONS:
            sig, desc = BUILTIN_FUNCTIONS[word]
            return Hover(
                contents=f"```kryos\n{sig}\n```\n\n{desc}"
            ).to_dict()

        # Check keywords
        if word in KEYWORD_DESCRIPTIONS:
            return Hover(
                contents=f"**`{word}`** (keyword)\n\n{KEYWORD_DESCRIPTIONS[word]}"
            ).to_dict()

        # Check built-in types
        if word in BUILTIN_TYPE_DESCRIPTIONS:
            return Hover(
                contents=f"**`{word}`** (built-in type)\n\n{BUILTIN_TYPE_DESCRIPTIONS[word]}"
            ).to_dict()

        # Check user-defined functions
        fn_info = doc.fn_defs.get(word)
        if fn_info is not None:
            decl, _ = fn_info
            if isinstance(decl, FnDecl):
                sig = self._fn_signature(decl)
                return Hover(contents=f"```kryos\n{sig}\n```").to_dict()

        # Check user-defined structs
        struct_info = doc.struct_defs.get(word)
        if struct_info is not None:
            decl, _ = struct_info
            fields = ", ".join(f"{f.name}: {self._type_str(f.type_annotation)}"
                               for f in decl.fields)
            return Hover(
                contents=f"```kryos\nstruct {decl.name} {{ {fields} }}\n```"
            ).to_dict()

        # Check user-defined enums
        enum_info = doc.enum_defs.get(word)
        if enum_info is not None:
            decl, _ = enum_info
            variants = ", ".join(v.name for v in decl.variants)
            return Hover(
                contents=f"```kryos\nenum {decl.name} {{ {variants} }}\n```"
            ).to_dict()

        return None

    # ------------------------------------------------------------------
    # Completion
    # ------------------------------------------------------------------

    def _on_completion(self, req_id: Any, params: Dict[str, Any]) -> Any:
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        doc = self._documents.get(uri)
        if doc is None:
            return []

        line_num = pos["line"]
        char_num = pos["character"]
        ctx = params.get("context", {})
        trigger = ctx.get("triggerCharacter", "")

        items: List[Dict[str, Any]] = []

        # Determine what the user typed so far on this line
        lines = doc.text.splitlines()
        line_text = lines[line_num] if line_num < len(lines) else ""
        prefix = line_text[:char_num]

        # After '@' -- attribute completion
        if trigger == "@" or prefix.rstrip().endswith("@"):
            for attr_name, desc in ATTRIBUTE_DESCRIPTIONS.items():
                items.append(CompletionItem(
                    label=f"@{attr_name}",
                    kind=CompletionItemKind.Keyword,
                    detail="attribute",
                    documentation=desc,
                    insert_text=attr_name,
                ).to_dict())
            return items

        # After '.' -- struct field completion
        if trigger == "." or (prefix and prefix[-1:] == "."):
            # Try to resolve the identifier before the dot
            ident = self._ident_before_dot(prefix)
            if ident and doc.module:
                items.extend(self._field_completions(doc, ident))
            # If we can't resolve, return empty -- don't pollute with keywords
            return items

        # General completion: keywords + built-in functions + built-in types + user defs
        # Keywords
        for kw, desc in KEYWORD_DESCRIPTIONS.items():
            items.append(CompletionItem(
                label=kw,
                kind=CompletionItemKind.Keyword,
                detail="keyword",
                documentation=desc,
            ).to_dict())

        # Built-in functions
        for name, (sig, desc) in BUILTIN_FUNCTIONS.items():
            items.append(CompletionItem(
                label=name,
                kind=CompletionItemKind.Function,
                detail=sig,
                documentation=desc,
            ).to_dict())

        # Built-in types
        for name, desc in BUILTIN_TYPE_DESCRIPTIONS.items():
            items.append(CompletionItem(
                label=name,
                kind=CompletionItemKind.Struct if name[0].isupper() else CompletionItemKind.TypeParameter,
                detail="built-in type",
                documentation=desc,
            ).to_dict())

        # User-defined functions
        for name, (decl, _) in doc.fn_defs.items():
            sig = self._fn_signature(decl) if isinstance(decl, FnDecl) else name
            items.append(CompletionItem(
                label=name,
                kind=CompletionItemKind.Function,
                detail=sig,
            ).to_dict())

        # User-defined structs
        for name, (decl, _) in doc.struct_defs.items():
            items.append(CompletionItem(
                label=name,
                kind=CompletionItemKind.Struct,
                detail=f"struct {name}",
            ).to_dict())

        # User-defined enums
        for name, (decl, _) in doc.enum_defs.items():
            items.append(CompletionItem(
                label=name,
                kind=CompletionItemKind.Enum,
                detail=f"enum {name}",
            ).to_dict())

        return items

    def _ident_before_dot(self, prefix: str) -> Optional[str]:
        """Extract the identifier immediately before a trailing dot."""
        text = prefix.rstrip()
        if not text or text[-1] != ".":
            return None
        text = text[:-1].rstrip()
        # Walk backwards to find the start of an identifier
        i = len(text) - 1
        while i >= 0 and (text[i].isalnum() or text[i] == "_"):
            i -= 1
        ident = text[i + 1:]
        return ident if ident else None

    def _field_completions(self, doc: DocumentState, ident: str) -> List[Dict[str, Any]]:
        """Generate field completions for a known struct type or variable."""
        items: List[Dict[str, Any]] = []

        # Direct struct name
        struct_info = doc.struct_defs.get(ident)
        if struct_info:
            decl, _ = struct_info
            for f in decl.fields:
                type_str = self._type_str(f.type_annotation)
                items.append(CompletionItem(
                    label=f.name,
                    kind=CompletionItemKind.Field,
                    detail=type_str,
                ).to_dict())
            return items

        # Try to find a let binding with this name and resolve its type
        if doc.module:
            resolved_type = self._resolve_variable_type(doc, ident)
            if resolved_type and resolved_type in doc.struct_defs:
                decl, _ = doc.struct_defs[resolved_type]
                for f in decl.fields:
                    type_str = self._type_str(f.type_annotation)
                    items.append(CompletionItem(
                        label=f.name,
                        kind=CompletionItemKind.Field,
                        detail=type_str,
                    ).to_dict())

        return items

    def _resolve_variable_type(self, doc: DocumentState, var_name: str) -> Optional[str]:
        """Best-effort: find a `let` binding for var_name and return its type name."""
        if doc.module is None:
            return None

        for decl in doc.module.declarations:
            if isinstance(decl, FnDecl) and decl.body:
                for stmt in decl.body.statements:
                    if isinstance(stmt, LetStmt) and stmt.name == var_name:
                        if stmt.type_annotation is not None:
                            return self._type_str(stmt.type_annotation)
                        # Check if RHS is a struct literal
                        from kryos.compiler.ast_nodes import StructLiteral
                        if isinstance(stmt.value, StructLiteral):
                            return stmt.value.type_name
        return None

    # ------------------------------------------------------------------
    # Go to Definition
    # ------------------------------------------------------------------

    def _on_definition(self, req_id: Any, params: Dict[str, Any]) -> Any:
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        doc = self._documents.get(uri)
        if doc is None:
            return None

        word = self._word_at(doc.text, pos["line"], pos["character"])
        if not word:
            return None

        # Search all open documents
        for d in self._documents.values():
            # Functions
            fn_info = d.fn_defs.get(word)
            if fn_info is not None:
                decl, def_uri = fn_info
                return self._span_to_location(def_uri, decl.span).to_dict()

            # Structs
            struct_info = d.struct_defs.get(word)
            if struct_info is not None:
                decl, def_uri = struct_info
                return self._span_to_location(def_uri, decl.span).to_dict()

            # Enums
            enum_info = d.enum_defs.get(word)
            if enum_info is not None:
                decl, def_uri = enum_info
                return self._span_to_location(def_uri, decl.span).to_dict()

        return None

    # ------------------------------------------------------------------
    # Document Symbols
    # ------------------------------------------------------------------

    def _on_document_symbol(self, req_id: Any, params: Dict[str, Any]) -> Any:
        uri = params["textDocument"]["uri"]
        doc = self._documents.get(uri)
        if doc is None or doc.module is None:
            return []

        symbols: List[Dict[str, Any]] = []

        for decl in doc.module.declarations:
            if isinstance(decl, FnDecl):
                sym = DocumentSymbol(
                    name=decl.name,
                    kind=SymbolKind.Function,
                    range=self._span_to_range(decl.span),
                    selection_range=self._span_to_range(decl.span),
                    detail=self._fn_signature(decl),
                )
                symbols.append(sym.to_dict())

            elif isinstance(decl, StructDecl):
                children = []
                for f in decl.fields:
                    child = DocumentSymbol(
                        name=f.name,
                        kind=SymbolKind.Field,
                        range=self._span_to_range(f.span),
                        selection_range=self._span_to_range(f.span),
                        detail=self._type_str(f.type_annotation),
                    )
                    children.append(child.to_dict())
                sym = DocumentSymbol(
                    name=decl.name,
                    kind=SymbolKind.Struct,
                    range=self._span_to_range(decl.span),
                    selection_range=self._span_to_range(decl.span),
                    children=[],  # will be set below
                )
                d = sym.to_dict()
                if children:
                    d["children"] = children
                symbols.append(d)

            elif isinstance(decl, EnumDecl):
                children = []
                for v in decl.variants:
                    child = DocumentSymbol(
                        name=v.name,
                        kind=SymbolKind.EnumMember,
                        range=self._span_to_range(v.span),
                        selection_range=self._span_to_range(v.span),
                    )
                    children.append(child.to_dict())
                sym = DocumentSymbol(
                    name=decl.name,
                    kind=SymbolKind.Enum,
                    range=self._span_to_range(decl.span),
                    selection_range=self._span_to_range(decl.span),
                )
                d = sym.to_dict()
                if children:
                    d["children"] = children
                symbols.append(d)

            elif isinstance(decl, TraitDecl):
                children = []
                for m in decl.methods:
                    child = DocumentSymbol(
                        name=m.name,
                        kind=SymbolKind.Method,
                        range=self._span_to_range(m.span),
                        selection_range=self._span_to_range(m.span),
                        detail=self._fn_signature(m),
                    )
                    children.append(child.to_dict())
                sym = DocumentSymbol(
                    name=decl.name,
                    kind=SymbolKind.Interface,
                    range=self._span_to_range(decl.span),
                    selection_range=self._span_to_range(decl.span),
                )
                d = sym.to_dict()
                if children:
                    d["children"] = children
                symbols.append(d)

            elif isinstance(decl, ActorDecl):
                sym = DocumentSymbol(
                    name=decl.name,
                    kind=SymbolKind.Class,
                    range=self._span_to_range(decl.span),
                    selection_range=self._span_to_range(decl.span),
                    detail="actor",
                )
                symbols.append(sym.to_dict())

            elif isinstance(decl, TypeAlias):
                sym = DocumentSymbol(
                    name=decl.name,
                    kind=SymbolKind.TypeParameter,
                    range=self._span_to_range(decl.span),
                    selection_range=self._span_to_range(decl.span),
                    detail="type alias",
                )
                symbols.append(sym.to_dict())

            elif isinstance(decl, ImplBlock):
                type_name = self._type_str(decl.target_type) if decl.target_type else "impl"
                children = []
                for m in decl.methods:
                    child = DocumentSymbol(
                        name=m.name,
                        kind=SymbolKind.Method,
                        range=self._span_to_range(m.span),
                        selection_range=self._span_to_range(m.span),
                        detail=self._fn_signature(m),
                    )
                    children.append(child.to_dict())
                sym = DocumentSymbol(
                    name=f"impl {type_name}",
                    kind=SymbolKind.Class,
                    range=self._span_to_range(decl.span),
                    selection_range=self._span_to_range(decl.span),
                )
                d = sym.to_dict()
                if children:
                    d["children"] = children
                symbols.append(d)

        return symbols

    # ------------------------------------------------------------------
    # Utility helpers
    # ------------------------------------------------------------------

    def _word_at(self, text: str, line: int, char: int) -> Optional[str]:
        """Extract the identifier at a given 0-based line/character position."""
        lines = text.splitlines()
        if line < 0 or line >= len(lines):
            return None
        ln = lines[line]
        if char < 0 or char >= len(ln):
            # Could be at end-of-line; try char-1
            if char > 0 and char == len(ln):
                char -= 1
            else:
                return None

        # Expand left
        start = char
        while start > 0 and (ln[start - 1].isalnum() or ln[start - 1] == "_"):
            start -= 1

        # Expand right
        end = char
        while end < len(ln) and (ln[end].isalnum() or ln[end] == "_"):
            end += 1

        word = ln[start:end]
        return word if word else None

    def _span_to_range(self, span: tuple) -> Range:
        """Convert a Kryos AST span (1-based) to an LSP Range (0-based)."""
        sl, sc, el, ec = span
        return Range(
            start=Position(max(sl - 1, 0), max(sc - 1, 0)),
            end=Position(max(el - 1, 0), max(ec, 0)),
        )

    def _span_to_location(self, uri: str, span: tuple) -> Location:
        return Location(uri=uri, range=self._span_to_range(span))

    def _fn_signature(self, decl) -> str:
        """Build a human-readable function signature string."""
        params_str = ", ".join(
            f"{p.name}: {self._type_str(p.type_annotation)}"
            if p.type_annotation else p.name
            for p in decl.params
        )
        ret = ""
        if decl.return_type is not None:
            ret = f" -> {self._type_str(decl.return_type)}"
        return f"fn {decl.name}({params_str}){ret}"

    def _type_str(self, type_node) -> str:
        """Convert a TypeNode to a readable string."""
        if type_node is None:
            return "unknown"
        from kryos.compiler.ast_nodes import SimpleType, GenericType, ArrayType, FnType, OptionType
        if isinstance(type_node, SimpleType):
            return type_node.name
        if isinstance(type_node, GenericType):
            params = ", ".join(self._type_str(t) for t in type_node.type_params)
            return f"{type_node.name}<{params}>"
        if isinstance(type_node, ArrayType):
            inner = self._type_str(type_node.element_type)
            if type_node.size is not None:
                return f"[{inner}; {type_node.size}]"
            return f"[{inner}]"
        if isinstance(type_node, FnType):
            params = ", ".join(self._type_str(t) for t in type_node.param_types)
            ret = self._type_str(type_node.return_type) if type_node.return_type else "void"
            return f"fn({params}) -> {ret}"
        if isinstance(type_node, OptionType):
            return f"{self._type_str(type_node.inner)}?"
        return str(type_node)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main() -> None:
    """Launch the Kryos language server."""
    server = KryosLanguageServer()
    server.run()


if __name__ == "__main__":
    main()
