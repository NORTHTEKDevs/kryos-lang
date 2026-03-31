"""
Kryos Language - Token Definitions
Defines all token types and the Token dataclass for the Kryos compiler.
"""

from enum import Enum, auto
from dataclasses import dataclass
from typing import Optional


class TokenType(Enum):
    """All token types recognized by the Kryos lexer."""

    # --- Literals ---
    INTEGER = auto()         # 42, 0xFF, 0b1010, 0o77, 1_000_000
    FLOAT = auto()           # 3.14, 1.0e10, 2.5e-3
    STRING = auto()          # "hello" (complete, no interpolation)
    STRING_PART = auto()     # segment of an interpolated string
    INTERP_START = auto()    # opening { inside an interpolated string
    INTERP_END = auto()      # closing } inside an interpolated string
    CHAR = auto()            # 'a', '\n'
    TRUE = auto()            # true
    FALSE = auto()           # false
    NONE = auto()            # none

    # --- Identifiers & Types ---
    IDENTIFIER = auto()      # user-defined names
    TYPE_IDENT = auto()      # built-in type identifiers (i32, Tensor, etc.)

    # --- Keywords ---
    LET = auto()
    MUT = auto()
    FN = auto()
    RETURN = auto()
    IF = auto()
    ELSE = auto()
    ELIF = auto()
    FOR = auto()
    WHILE = auto()
    IN = auto()
    BREAK = auto()
    CONTINUE = auto()
    STRUCT = auto()
    ENUM = auto()
    IMPL = auto()
    TRAIT = auto()
    PUB = auto()
    USE = auto()
    EXTERN = auto()
    AS = auto()
    MOD = auto()
    TYPE = auto()
    ACTOR = auto()
    SPAWN = auto()
    PARALLEL = auto()
    QUANTUM = auto()
    COMPTIME = auto()
    AND = auto()
    OR = auto()
    NOT = auto()

    # --- Attribute keywords ---
    AT_CAPABILITIES = auto()    # @capabilities
    AT_COMPUTE = auto()         # @compute
    AT_EXPORT = auto()          # @export
    AT_LAYOUT = auto()          # @layout
    AT_REAL_TIME = auto()       # @real_time
    AT_NO_STD = auto()          # @no_std
    AT_ZERO_COPY = auto()       # @zero_copy
    AT_TARGET = auto()          # @target
    AT_DIFFERENTIABLE = auto()  # @differentiable

    # --- Arithmetic operators ---
    PLUS = auto()        # +
    MINUS = auto()       # -
    STAR = auto()        # *
    SLASH = auto()       # /
    PERCENT = auto()     # %
    POWER = auto()       # **
    AT = auto()          # @ (matrix mul, also attribute prefix)

    # --- Comparison operators ---
    EQ_EQ = auto()       # ==
    BANG_EQ = auto()      # !=
    LT = auto()          # <
    GT = auto()          # >
    LT_EQ = auto()       # <=
    GT_EQ = auto()       # >=

    # --- Assignment operators ---
    EQ = auto()          # =
    PLUS_EQ = auto()     # +=
    MINUS_EQ = auto()    # -=
    STAR_EQ = auto()     # *=
    SLASH_EQ = auto()    # /=

    # --- Bitwise operators ---
    AMP = auto()         # &
    PIPE = auto()        # |
    CARET = auto()       # ^
    TILDE = auto()       # ~
    SHL = auto()         # <<
    SHR = auto()         # >>

    # --- Punctuation / delimiters ---
    ARROW = auto()       # ->
    FAT_ARROW = auto()   # =>
    COLON_COLON = auto() # ::
    DOT_DOT = auto()     # ..
    DOT_DOT_EQ = auto()  # ..=
    DOT = auto()         # .
    COLON = auto()       # :
    SEMICOLON = auto()   # ;
    COMMA = auto()       # ,

    # --- Grouping ---
    LPAREN = auto()      # (
    RPAREN = auto()      # )
    LBRACE = auto()      # {
    RBRACE = auto()      # }
    LBRACKET = auto()    # [
    RBRACKET = auto()    # ]

    # --- Special ---
    NEWLINE = auto()     # significant newlines (if needed later)
    EOF = auto()         # end of file
    ERROR = auto()       # lexer error token


@dataclass(frozen=True, slots=True)
class Token:
    """A single token produced by the Kryos lexer."""
    type: TokenType
    value: str
    line: int
    column: int
    file: str = "<unknown>"

    def __repr__(self) -> str:
        loc = f"{self.file}:{self.line}:{self.column}"
        return f"Token({self.type.name}, {self.value!r}, {loc})"


# ---------------------------------------------------------------------------
# Keyword and type lookup tables
# ---------------------------------------------------------------------------

KEYWORDS: dict[str, TokenType] = {
    "let":      TokenType.LET,
    "mut":      TokenType.MUT,
    "fn":       TokenType.FN,
    "return":   TokenType.RETURN,
    "if":       TokenType.IF,
    "else":     TokenType.ELSE,
    "elif":     TokenType.ELIF,
    "for":      TokenType.FOR,
    "while":    TokenType.WHILE,
    "in":       TokenType.IN,
    "break":    TokenType.BREAK,
    "continue": TokenType.CONTINUE,
    "struct":   TokenType.STRUCT,
    "enum":     TokenType.ENUM,
    "impl":     TokenType.IMPL,
    "trait":    TokenType.TRAIT,
    "pub":      TokenType.PUB,
    "use":      TokenType.USE,
    "extern":   TokenType.EXTERN,
    "as":       TokenType.AS,
    "mod":      TokenType.MOD,
    "type":     TokenType.TYPE,
    "true":     TokenType.TRUE,
    "false":    TokenType.FALSE,
    "none":     TokenType.NONE,
    "actor":    TokenType.ACTOR,
    "spawn":    TokenType.SPAWN,
    "parallel": TokenType.PARALLEL,
    "quantum":  TokenType.QUANTUM,
    "comptime": TokenType.COMPTIME,
    "and":      TokenType.AND,
    "or":       TokenType.OR,
    "not":      TokenType.NOT,
}

ATTRIBUTE_KEYWORDS: dict[str, TokenType] = {
    "capabilities":   TokenType.AT_CAPABILITIES,
    "compute":        TokenType.AT_COMPUTE,
    "export":         TokenType.AT_EXPORT,
    "layout":         TokenType.AT_LAYOUT,
    "real_time":      TokenType.AT_REAL_TIME,
    "no_std":         TokenType.AT_NO_STD,
    "zero_copy":      TokenType.AT_ZERO_COPY,
    "target":         TokenType.AT_TARGET,
    "differentiable": TokenType.AT_DIFFERENTIABLE,
}

BUILTIN_TYPES: set[str] = {
    # Integer types
    "i8", "i16", "i32", "i64", "i128",
    "u8", "u16", "u32", "u64", "u128",
    # Float types
    "f32", "f64",
    # Primitives
    "bool", "str", "char",
    # Compound / domain types
    "Tensor", "Vec", "Map", "Set", "Option", "Result", "Secret", "Qubit", "Qureg",
}
