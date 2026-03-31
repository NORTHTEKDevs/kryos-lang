"""
Kryos LSP -- JSON-RPC message parsing/encoding and LSP type definitions.

Implements the subset of the Language Server Protocol needed by the Kryos
language server.  Pure Python stdlib -- no pip dependencies.
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass, field, asdict
from typing import Any, Dict, List, Optional, Union


# ---------------------------------------------------------------------------
# JSON-RPC transport
# ---------------------------------------------------------------------------

def read_message(stream=None) -> Optional[Dict[str, Any]]:
    """
    Read a single JSON-RPC message from *stream* (default ``sys.stdin.buffer``).

    Returns the parsed JSON dict, or ``None`` on EOF.
    """
    if stream is None:
        stream = sys.stdin.buffer

    # Read headers until blank line
    headers: Dict[str, str] = {}
    while True:
        line = stream.readline()
        if not line:
            return None  # EOF
        line_str = line.decode("ascii", errors="replace").rstrip("\r\n")
        if line_str == "":
            break  # end of headers
        if ":" in line_str:
            key, _, val = line_str.partition(":")
            headers[key.strip().lower()] = val.strip()

    content_length = int(headers.get("content-length", "0"))
    if content_length == 0:
        return None

    body = stream.read(content_length)
    if not body:
        return None

    return json.loads(body.decode("utf-8"))


def write_message(msg: Dict[str, Any], stream=None) -> None:
    """
    Write a single JSON-RPC message to *stream* (default ``sys.stdout.buffer``).
    """
    if stream is None:
        stream = sys.stdout.buffer

    body = json.dumps(msg, ensure_ascii=False).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
    stream.write(header)
    stream.write(body)
    stream.flush()


def make_response(req_id: Union[int, str, None], result: Any) -> Dict[str, Any]:
    """Build a JSON-RPC success response."""
    return {"jsonrpc": "2.0", "id": req_id, "result": result}


def make_error(req_id: Union[int, str, None], code: int, message: str,
               data: Any = None) -> Dict[str, Any]:
    """Build a JSON-RPC error response."""
    err: Dict[str, Any] = {"code": code, "message": message}
    if data is not None:
        err["data"] = data
    return {"jsonrpc": "2.0", "id": req_id, "error": err}


def make_notification(method: str, params: Any) -> Dict[str, Any]:
    """Build a JSON-RPC notification (no id)."""
    return {"jsonrpc": "2.0", "method": method, "params": params}


# ---------------------------------------------------------------------------
# JSON-RPC error codes
# ---------------------------------------------------------------------------

PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603
SERVER_NOT_INITIALIZED = -32002
REQUEST_CANCELLED = -32800


# ---------------------------------------------------------------------------
# LSP Position / Range / Location
# ---------------------------------------------------------------------------

@dataclass
class Position:
    """Zero-based line and character offset."""
    line: int = 0
    character: int = 0

    def to_dict(self) -> Dict[str, int]:
        return {"line": self.line, "character": self.character}


@dataclass
class Range:
    """A range in a text document (start inclusive, end exclusive)."""
    start: Position = field(default_factory=Position)
    end: Position = field(default_factory=Position)

    def to_dict(self) -> Dict[str, Any]:
        return {"start": self.start.to_dict(), "end": self.end.to_dict()}


@dataclass
class Location:
    uri: str = ""
    range: Range = field(default_factory=Range)

    def to_dict(self) -> Dict[str, Any]:
        return {"uri": self.uri, "range": self.range.to_dict()}


# ---------------------------------------------------------------------------
# Diagnostics
# ---------------------------------------------------------------------------

class DiagnosticSeverity:
    Error = 1
    Warning = 2
    Information = 3
    Hint = 4


@dataclass
class Diagnostic:
    range: Range = field(default_factory=Range)
    severity: int = DiagnosticSeverity.Error
    source: str = "kryos"
    message: str = ""

    def to_dict(self) -> Dict[str, Any]:
        return {
            "range": self.range.to_dict(),
            "severity": self.severity,
            "source": self.source,
            "message": self.message,
        }


# ---------------------------------------------------------------------------
# Completion
# ---------------------------------------------------------------------------

class CompletionItemKind:
    Text = 1
    Method = 2
    Function = 3
    Constructor = 4
    Field = 5
    Variable = 6
    Class = 7
    Interface = 8
    Module = 9
    Property = 10
    Unit = 11
    Value = 12
    Enum = 13
    Keyword = 14
    Snippet = 15
    Color = 16
    File = 17
    Reference = 18
    Folder = 19
    EnumMember = 20
    Constant = 21
    Struct = 22
    Event = 23
    Operator = 24
    TypeParameter = 25


@dataclass
class CompletionItem:
    label: str = ""
    kind: int = CompletionItemKind.Text
    detail: str = ""
    documentation: str = ""
    insert_text: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        d: Dict[str, Any] = {
            "label": self.label,
            "kind": self.kind,
        }
        if self.detail:
            d["detail"] = self.detail
        if self.documentation:
            d["documentation"] = {"kind": "markdown", "value": self.documentation}
        if self.insert_text is not None:
            d["insertText"] = self.insert_text
        return d


# ---------------------------------------------------------------------------
# Document Symbols
# ---------------------------------------------------------------------------

class SymbolKind:
    File = 1
    Module = 2
    Namespace = 3
    Package = 4
    Class = 5
    Method = 6
    Property = 7
    Field = 8
    Constructor = 9
    Enum = 10
    Interface = 11
    Function = 12
    Variable = 13
    Constant = 14
    String = 15
    Number = 16
    Boolean = 17
    Array = 18
    Object = 19
    Key = 20
    Null = 21
    EnumMember = 22
    Struct = 23
    Event = 24
    Operator = 25
    TypeParameter = 26


@dataclass
class DocumentSymbol:
    name: str = ""
    kind: int = SymbolKind.Function
    range: Range = field(default_factory=Range)
    selection_range: Range = field(default_factory=Range)
    detail: str = ""
    children: List["DocumentSymbol"] = field(default_factory=list)

    def to_dict(self) -> Dict[str, Any]:
        d: Dict[str, Any] = {
            "name": self.name,
            "kind": self.kind,
            "range": self.range.to_dict(),
            "selectionRange": self.selection_range.to_dict(),
        }
        if self.detail:
            d["detail"] = self.detail
        if self.children:
            d["children"] = [c.to_dict() for c in self.children]
        return d


# ---------------------------------------------------------------------------
# Hover
# ---------------------------------------------------------------------------

@dataclass
class Hover:
    contents: str = ""
    range: Optional[Range] = None

    def to_dict(self) -> Dict[str, Any]:
        d: Dict[str, Any] = {
            "contents": {"kind": "markdown", "value": self.contents},
        }
        if self.range is not None:
            d["range"] = self.range.to_dict()
        return d


# ---------------------------------------------------------------------------
# TextEdit helpers
# ---------------------------------------------------------------------------

class TextDocumentSyncKind:
    None_ = 0
    Full = 1
    Incremental = 2
