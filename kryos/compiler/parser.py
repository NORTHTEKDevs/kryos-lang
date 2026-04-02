"""
Kryos Language - Recursive Descent / Pratt Parser
Transforms a token list into an AST (Module node).

Usage:
    from kryos.compiler.parser import parse
    module = parse(tokens)           # tokens: list[Token]
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from kryos.compiler.tokens import TokenType, Token
from kryos.compiler.ast_nodes import (
    ASTNode, Span, NO_SPAN, Module,
    # Type nodes
    TypeNode, SimpleType, GenericType, ArrayType, FnType, OptionType,
    # Declarations
    Declaration, GenericParam, FnDecl, StructDecl, StructField,
    EnumDecl, EnumVariant, TraitDecl, ImplBlock, ActorDecl, MessageHandler,
    TypeAlias, ImportDecl, ImportPath,
    # Statements
    Statement, BlockStmt, LetStmt, AssignStmt, ReturnStmt,
    IfStmt, ElifClause, ForStmt, WhileStmt, BreakStmt, ContinueStmt,
    ExprStmt, SpawnStmt, SelectStmt, SelectBranch, TryCatchStmt, ThrowStmt,
    # Expressions
    Expression, IntLiteral, FloatLiteral, StringLiteral, CharLiteral,
    BoolLiteral, NoneLiteral, Identifier,
    BinaryOp, UnaryOp, FnCall, MethodCall, FieldAccess, IndexAccess,
    ArrayLiteral, MapLiteral, StructLiteral, Lambda, Parameter,
    RangeExpr, IfExpr, MatchExpr, MatchArm, PipeExpr,
    QuantumBlock, ComptimeBlock,
    Attribute,
)


# ═══════════════════════════════════════════════════════════════════════════
# Errors
# ═══════════════════════════════════════════════════════════════════════════

@dataclass
class ParseError(Exception):
    """A single parse error with location information."""
    message: str
    file: str = "<unknown>"
    line: int = 0
    column: int = 0

    def __str__(self) -> str:
        return f"{self.file}:{self.line}:{self.column}: {self.message}"


class ParseErrors(Exception):
    """Aggregated collection of parse errors (for multi-error reporting)."""

    def __init__(self, errors: list[ParseError]) -> None:
        self.errors = errors
        super().__init__(self._format())

    def _format(self) -> str:
        lines = [f"Found {len(self.errors)} parse error(s):"]
        for e in self.errors:
            lines.append(f"  {e}")
        return "\n".join(lines)


# ═══════════════════════════════════════════════════════════════════════════
# Token stream helpers
# ═══════════════════════════════════════════════════════════════════════════

_TT = TokenType  # short alias used throughout

# Tokens that can begin a statement / declaration (used for error recovery).
_SYNC_TOKENS = frozenset({
    _TT.FN, _TT.LET, _TT.RETURN, _TT.IF, _TT.FOR, _TT.WHILE,
    _TT.STRUCT, _TT.ENUM, _TT.TRAIT, _TT.IMPL, _TT.ACTOR,
    _TT.USE, _TT.TYPE, _TT.BREAK, _TT.CONTINUE, _TT.AT,
    _TT.PUB, _TT.RBRACE, _TT.EOF, _TT.TRY, _TT.THROW, _TT.MATCH,
    _TT.SELECT,
})

# Compound-assignment operators
_ASSIGN_OPS = frozenset({
    _TT.EQ, _TT.PLUS_EQ, _TT.MINUS_EQ, _TT.STAR_EQ, _TT.SLASH_EQ,
})

# Map token type -> human-readable string for error messages.
_TOKEN_NAMES: dict[TokenType, str] = {
    _TT.LPAREN: "'('", _TT.RPAREN: "')'",
    _TT.LBRACE: "'{'", _TT.RBRACE: "'}'",
    _TT.LBRACKET: "'['", _TT.RBRACKET: "']'",
    _TT.COMMA: "','", _TT.COLON: "':'",
    _TT.SEMICOLON: "';'", _TT.DOT: "'.'",
    _TT.ARROW: "'->'", _TT.FAT_ARROW: "'=>'",
    _TT.EQ: "'='", _TT.EOF: "EOF",
    _TT.IDENTIFIER: "identifier",
    _TT.PIPE: "'|'",
    _TT.LT: "'<'", _TT.GT: "'>'",
}


def _tok_name(tt: TokenType) -> str:
    return _TOKEN_NAMES.get(tt, f"'{tt.name}'")


# ═══════════════════════════════════════════════════════════════════════════
# Parser
# ═══════════════════════════════════════════════════════════════════════════

class Parser:
    """Recursive-descent parser with Pratt-style expression parsing."""

    def __init__(self, tokens: list[Token]) -> None:
        # Filter out newlines -- Kryos is not newline-sensitive at parser level.
        self._tokens: list[Token] = [
            t for t in tokens if t.type != _TT.NEWLINE
        ]
        self._pos: int = 0
        self.errors: list[ParseError] = []

    # -------------------------------------------------------------------
    # Cursor helpers
    # -------------------------------------------------------------------

    def _cur(self) -> Token:
        if self._pos < len(self._tokens):
            return self._tokens[self._pos]
        return self._tokens[-1]  # EOF sentinel

    def _peek(self, offset: int = 0) -> Token:
        idx = self._pos + offset
        if 0 <= idx < len(self._tokens):
            return self._tokens[idx]
        return self._tokens[-1]

    def _at(self, *types: TokenType) -> bool:
        return self._cur().type in types

    def _at_ahead(self, offset: int, *types: TokenType) -> bool:
        return self._peek(offset).type in types

    def _advance(self) -> Token:
        tok = self._cur()
        if tok.type != _TT.EOF:
            self._pos += 1
        return tok

    def _expect(self, tt: TokenType, context: str = "") -> Token:
        tok = self._cur()
        if tok.type == tt:
            return self._advance()
        msg = f"Expected {_tok_name(tt)}"
        if context:
            msg += f" {context}"
        msg += f", got {_tok_name(tok.type)}"
        if tok.value and tok.type not in (_TT.EOF, _TT.ERROR):
            msg += f" ({tok.value!r})"
        self._error(msg, tok)
        # Return a synthetic token so callers can continue.
        return Token(tt, "", tok.line, tok.column, tok.file)

    def _match(self, *types: TokenType) -> Optional[Token]:
        if self._cur().type in types:
            return self._advance()
        return None

    # -------------------------------------------------------------------
    # Span helpers
    # -------------------------------------------------------------------

    def _span(self, tok: Token) -> Span:
        """Create a span covering a single token."""
        return (tok.line, tok.column, tok.line, tok.column + len(tok.value))

    def _span_from(self, start: Token) -> Span:
        """Create a span from *start* to the token just consumed."""
        prev = self._peek(-1) if self._pos > 0 else start
        return (start.line, start.column, prev.line, prev.column + len(prev.value))

    # -------------------------------------------------------------------
    # Error handling & recovery
    # -------------------------------------------------------------------

    def _error(self, message: str, tok: Token | None = None) -> None:
        if tok is None:
            tok = self._cur()
        self.errors.append(ParseError(message, tok.file, tok.line, tok.column))

    def _synchronize(self) -> None:
        """Skip tokens until we reach something that looks like a new statement."""
        self._advance()
        while not self._at(_TT.EOF):
            if self._cur().type in _SYNC_TOKENS:
                return
            self._advance()

    # ===================================================================
    # Top level
    # ===================================================================

    def parse_module(self) -> Module:
        start = self._cur()
        decls: list[Declaration] = []
        while not self._at(_TT.EOF):
            try:
                node = self._declaration()
                if node is not None:
                    decls.append(node)
            except ParseError:
                self._synchronize()
        return Module(span=self._span_from(start), declarations=decls)

    # ===================================================================
    # Declarations
    # ===================================================================

    def _declaration(self) -> Declaration | None:
        attrs = self._parse_attributes()

        is_pub = bool(self._match(_TT.PUB))

        if self._at(_TT.FN) and self._at_ahead(1, _TT.IDENTIFIER):
            return self._fn_decl(attrs)
        if self._at(_TT.STRUCT):
            return self._struct_decl(attrs)
        if self._at(_TT.ENUM):
            return self._enum_decl(attrs)
        if self._at(_TT.TRAIT):
            return self._trait_decl(attrs)
        if self._at(_TT.IMPL):
            return self._impl_block(attrs)
        if self._at(_TT.ACTOR):
            return self._actor_decl(attrs)
        if self._at(_TT.USE):
            return self._import_decl()
        if self._at(_TT.TYPE):
            return self._type_alias(attrs)

        if attrs:
            self._error("Attributes must be followed by a declaration")

        # Fall through to a statement wrapped in a synthetic FnDecl-free zone.
        # At top level, statements are allowed; wrap them.
        stmt = self._statement()
        if stmt is not None:
            # Wrap statement as a pseudo-declaration so it fits in Module.declarations.
            # ExprStmt and other Statement subclasses inherit from ASTNode but not
            # Declaration. We create a thin wrapper via a cast -- the Module.declarations
            # type hint is List[Declaration] but at runtime any ASTNode works.
            return stmt  # type: ignore[return-value]
        return None

    # -------------------------------------------------------------------
    # Attributes   @name  |  @name(args)
    # -------------------------------------------------------------------

    # Map AT_* token types to attribute names
    _ATTR_TOKENS = {
        _TT.AT_CAPABILITIES: "capabilities",
        _TT.AT_COMPUTE: "compute",
        _TT.AT_EXPORT: "export",
        _TT.AT_LAYOUT: "layout",
        _TT.AT_REAL_TIME: "real_time",
        _TT.AT_NO_STD: "no_std",
        _TT.AT_ZERO_COPY: "zero_copy",
        _TT.AT_TARGET: "target",
        _TT.AT_DIFFERENTIABLE: "differentiable",
    }

    def _parse_attributes(self) -> list[Attribute]:
        attrs: list[Attribute] = []
        while self._at(_TT.AT, *self._ATTR_TOKENS.keys()):
            if self._at(_TT.AT):
                # Generic @identifier attribute (also accept keywords as attr names)
                start = self._advance()  # consume '@'
                tok = self._cur()
                if tok.type == _TT.IDENTIFIER or tok.type in self._KEYWORD_AS_IDENT:
                    name_tok = self._advance()
                    name = name_tok.value
                else:
                    name_tok = self._expect(_TT.IDENTIFIER, "after '@'")
                    name = name_tok.value
            else:
                # Compound AT_* token (e.g., AT_CAPABILITIES)
                start = self._advance()
                name = self._ATTR_TOKENS[start.type]
            args: list[Expression] = []
            if self._match(_TT.LPAREN):
                args = self._parse_attr_args()
                self._expect(_TT.RPAREN, "to close attribute arguments")
            attrs.append(Attribute(
                span=self._span_from(start), name=name, args=args,
            ))
        return attrs

    # Keyword tokens that should be treated as identifiers in attribute arguments
    _KEYWORD_AS_IDENT = {
        _TT.QUANTUM, _TT.PARALLEL, _TT.ACTOR, _TT.SPAWN, _TT.SELECT, _TT.COMPTIME,
        _TT.FN, _TT.STRUCT, _TT.ENUM, _TT.TRAIT, _TT.IMPL, _TT.PUB,
        _TT.MOD, _TT.TYPE, _TT.MUT, _TT.USE, _TT.EXTERN, _TT.AS,
        _TT.TRUE, _TT.FALSE, _TT.NONE, _TT.AND, _TT.OR, _TT.NOT,
        _TT.LET, _TT.IF, _TT.ELSE, _TT.ELIF, _TT.FOR, _TT.WHILE,
        _TT.IN, _TT.BREAK, _TT.CONTINUE, _TT.RETURN,
        _TT.TRY, _TT.CATCH, _TT.THROW, _TT.MATCH,
        _TT.SEND, _TT.RECV, _TT.ASK, _TT.CHAN,
    }

    def _parse_attr_args(self) -> list[Expression]:
        """Parse attribute arguments, treating keywords as identifiers."""
        args: list[Expression] = []
        while not self._at(_TT.RPAREN, _TT.EOF):
            tok = self._cur()
            if tok.type == _TT.STRING:
                # String literal argument
                self._advance()
                args.append(StringLiteral(span=self._span(tok), value=tok.value))
            elif tok.type == _TT.INTEGER:
                self._advance()
                args.append(IntLiteral(span=self._span(tok), value=int(tok.value)))
            elif tok.type == _TT.FLOAT:
                self._advance()
                args.append(FloatLiteral(span=self._span(tok), value=float(tok.value)))
            elif tok.type in (_TT.IDENTIFIER, _TT.TYPE_IDENT) or tok.type in self._KEYWORD_AS_IDENT:
                # Identifier or keyword-as-identifier
                self._advance()
                node = Identifier(span=self._span(tok), name=tok.value)
                # Check for colon: sub-capability (network:raw_socket) or
                # named parameter (max_cpu: "10s", paths: [...])
                if self._match(_TT.COLON):
                    sub_tok = self._cur()
                    if sub_tok.type in (_TT.IDENTIFIER, _TT.TYPE_IDENT) or sub_tok.type in self._KEYWORD_AS_IDENT:
                        self._advance()
                        node = FieldAccess(
                            span=self._span(tok), object=node, field=sub_tok.value,
                        )
                    elif sub_tok.type in (_TT.STRING, _TT.INTEGER, _TT.FLOAT, _TT.LBRACKET):
                        # Named parameter with literal value: key: "value" / key: 42 / key: [...]
                        val_expr = self._expression()
                        node = FieldAccess(
                            span=self._span(tok), object=node, field=str(getattr(val_expr, 'value', val_expr)),
                        )
                args.append(node)
            else:
                # Fall back to expression parsing
                args.append(self._expression())

            if not self._match(_TT.COMMA):
                break
        return args

    # -------------------------------------------------------------------
    # fn_decl
    # -------------------------------------------------------------------

    def _fn_decl(self, attrs: list[Attribute] | None = None) -> FnDecl:
        start = self._advance()  # consume 'fn'
        name_tok = self._expect(_TT.IDENTIFIER, "as function name")
        generics = self._generic_params()
        self._expect(_TT.LPAREN, "to open parameter list")
        params = self._param_list()
        self._expect(_TT.RPAREN, "to close parameter list")

        ret_type: TypeNode | None = None
        if self._match(_TT.ARROW):
            ret_type = self._type_expr()

        body: BlockStmt | None = None
        if self._at(_TT.LBRACE):
            body = self._block()

        return FnDecl(
            span=self._span_from(start),
            name=name_tok.value,
            params=params,
            return_type=ret_type,
            body=body,
            generics=generics,
            attributes=attrs or [],
        )

    def _param_list(self) -> list[Parameter]:
        params: list[Parameter] = []
        while not self._at(_TT.RPAREN, _TT.EOF):
            params.append(self._parameter())
            if not self._match(_TT.COMMA):
                break
        return params

    def _parameter(self) -> Parameter:
        start = self._cur()
        name_tok = self._expect(_TT.IDENTIFIER, "as parameter name")
        type_ann: TypeNode | None = None
        default: Expression | None = None
        if self._match(_TT.COLON):
            type_ann = self._type_expr()
        if self._match(_TT.EQ):
            default = self._expression()
        return Parameter(
            span=self._span_from(start),
            name=name_tok.value,
            type_annotation=type_ann,
            default_value=default,
        )

    # -------------------------------------------------------------------
    # struct_decl
    # -------------------------------------------------------------------

    def _struct_decl(self, attrs: list[Attribute]) -> StructDecl:
        start = self._advance()  # consume 'struct'
        name_tok = self._expect(_TT.IDENTIFIER, "as struct name")
        generics = self._generic_params()
        self._expect(_TT.LBRACE, f"to open struct '{name_tok.value}'")
        fields = self._struct_fields()
        self._expect(
            _TT.RBRACE,
            f"to close struct '{name_tok.value}' started at line {start.line}",
        )
        return StructDecl(
            span=self._span_from(start),
            name=name_tok.value,
            fields=fields,
            generics=generics,
            attributes=attrs,
        )

    def _struct_fields(self) -> list[StructField]:
        fields: list[StructField] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            f_attrs = self._parse_attributes()
            start = self._cur()
            name_tok = self._expect(_TT.IDENTIFIER, "as field name")
            self._expect(_TT.COLON, "after field name")
            type_ann = self._type_expr()
            default: Expression | None = None
            if self._match(_TT.EQ):
                default = self._expression()
            fields.append(StructField(
                span=self._span_from(start),
                name=name_tok.value,
                type_annotation=type_ann,
                default_value=default,
                attributes=f_attrs,
            ))
            if not self._match(_TT.COMMA):
                break
        return fields

    # -------------------------------------------------------------------
    # enum_decl
    # -------------------------------------------------------------------

    def _enum_decl(self, attrs: list[Attribute]) -> EnumDecl:
        start = self._advance()  # consume 'enum'
        name_tok = self._expect(_TT.IDENTIFIER, "as enum name")
        generics = self._generic_params()
        self._expect(_TT.LBRACE, f"to open enum '{name_tok.value}'")
        variants = self._enum_variants()
        self._expect(
            _TT.RBRACE,
            f"to close enum '{name_tok.value}' started at line {start.line}",
        )
        return EnumDecl(
            span=self._span_from(start),
            name=name_tok.value,
            variants=variants,
            generics=generics,
            attributes=attrs,
        )

    def _enum_variants(self) -> list[EnumVariant]:
        variants: list[EnumVariant] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            v_attrs = self._parse_attributes()
            start = self._cur()
            name_tok = self._expect(_TT.IDENTIFIER, "as variant name")
            field_types: list[TypeNode] = []
            if self._match(_TT.LPAREN):
                while not self._at(_TT.RPAREN, _TT.EOF):
                    field_types.append(self._type_expr())
                    if not self._match(_TT.COMMA):
                        break
                self._expect(_TT.RPAREN, "to close variant fields")
            variants.append(EnumVariant(
                span=self._span_from(start),
                name=name_tok.value,
                fields=field_types,
                attributes=v_attrs,
            ))
            if not self._match(_TT.COMMA):
                break
        return variants

    # -------------------------------------------------------------------
    # trait_decl
    # -------------------------------------------------------------------

    def _trait_decl(self, attrs: list[Attribute]) -> TraitDecl:
        start = self._advance()  # consume 'trait'
        name_tok = self._expect(_TT.IDENTIFIER, "as trait name")
        generics = self._generic_params()
        self._expect(_TT.LBRACE, f"to open trait '{name_tok.value}'")
        methods: list[FnDecl] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            fn_attrs = self._parse_attributes()
            if self._at(_TT.FN):
                methods.append(self._fn_decl(fn_attrs))
            else:
                self._error("Expected 'fn' inside trait body")
                self._synchronize()
        self._expect(
            _TT.RBRACE,
            f"to close trait '{name_tok.value}' started at line {start.line}",
        )
        return TraitDecl(
            span=self._span_from(start),
            name=name_tok.value,
            methods=methods,
            generics=generics,
            attributes=attrs,
        )

    # -------------------------------------------------------------------
    # impl_block
    # -------------------------------------------------------------------

    def _impl_block(self, attrs: list[Attribute]) -> ImplBlock:
        start = self._advance()  # consume 'impl'
        first_type = self._type_expr()
        trait_type: TypeNode | None = None
        target_type: TypeNode

        if self._match(_TT.FOR):
            # impl Trait for Type
            trait_type = first_type
            target_type = self._type_expr()
        else:
            target_type = first_type

        self._expect(_TT.LBRACE, "to open impl block")
        methods: list[FnDecl] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            fn_attrs = self._parse_attributes()
            _ = self._match(_TT.PUB)  # consume optional pub
            if self._at(_TT.FN):
                methods.append(self._fn_decl(fn_attrs))
            else:
                self._error("Expected 'fn' inside impl block")
                self._synchronize()
        self._expect(
            _TT.RBRACE,
            f"to close impl block started at line {start.line}",
        )
        return ImplBlock(
            span=self._span_from(start),
            target_type=target_type,
            trait_type=trait_type,
            methods=methods,
            attributes=attrs,
        )

    # -------------------------------------------------------------------
    # actor_decl
    # -------------------------------------------------------------------

    def _actor_decl(self, attrs: list[Attribute]) -> ActorDecl:
        start = self._advance()  # consume 'actor'
        name_tok = self._expect(_TT.IDENTIFIER, "as actor name")
        self._expect(_TT.LBRACE, f"to open actor '{name_tok.value}'")

        state_fields: list[StructField] = []
        handlers: list[MessageHandler] = []

        while not self._at(_TT.RBRACE, _TT.EOF):
            if self._at(_TT.LET):
                # State field: let name: type = value
                let_stmt = self._let_stmt()
                state_fields.append(StructField(
                    span=let_stmt.span,
                    name=let_stmt.name,
                    type_annotation=let_stmt.type_annotation,
                    default_value=let_stmt.value,
                ))
            elif self._at(_TT.FN):
                fn = self._fn_decl()
                handlers.append(MessageHandler(
                    span=fn.span,
                    name=fn.name,
                    params=fn.params,
                    return_type=fn.return_type,
                    body=fn.body,
                    attributes=fn.attributes,
                ))
            else:
                self._error("Expected 'let' or 'fn' inside actor body")
                self._synchronize()

        self._expect(
            _TT.RBRACE,
            f"to close actor '{name_tok.value}' started at line {start.line}",
        )
        return ActorDecl(
            span=self._span_from(start),
            name=name_tok.value,
            state_fields=state_fields,
            handlers=handlers,
            attributes=attrs,
        )

    # -------------------------------------------------------------------
    # import_decl  (use ...)
    # -------------------------------------------------------------------

    def _import_decl(self) -> ImportDecl:
        start = self._advance()  # consume 'use'
        is_extern = False
        extern_source: str | None = None

        if self._match(_TT.EXTERN):
            is_extern = True
            ext_tok = self._expect(_TT.STRING, "as extern source path")
            extern_source = ext_tok.value

        path = self._import_path()
        return ImportDecl(
            span=self._span_from(start),
            path=path,
            is_extern=is_extern,
            extern_source=extern_source,
        )

    def _import_path(self) -> ImportPath:
        start = self._cur()
        segments: list[str] = []
        tok = self._expect(_TT.IDENTIFIER, "in import path")
        segments.append(tok.value)
        while self._match(_TT.COLON_COLON):
            seg = self._expect(_TT.IDENTIFIER, "in import path")
            segments.append(seg.value)
        alias: str | None = None
        if self._match(_TT.AS):
            alias_tok = self._expect(_TT.IDENTIFIER, "as alias name")
            alias = alias_tok.value
        return ImportPath(
            span=self._span_from(start),
            segments=segments,
            alias=alias,
        )

    # -------------------------------------------------------------------
    # type_alias
    # -------------------------------------------------------------------

    def _type_alias(self, attrs: list[Attribute]) -> TypeAlias:
        start = self._advance()  # consume 'type'
        name_tok = self._expect(_TT.IDENTIFIER, "as type alias name")
        generics = self._generic_params()
        self._expect(_TT.EQ, "after type alias name")
        value = self._type_expr()
        return TypeAlias(
            span=self._span_from(start),
            name=name_tok.value,
            generics=generics,
            value=value,
            attributes=attrs,
        )

    # ===================================================================
    # Generics  <T, U: Trait>
    # ===================================================================

    def _generic_params(self) -> list[GenericParam]:
        if not self._match(_TT.LT):
            return []
        params: list[GenericParam] = []
        while not self._at(_TT.GT, _TT.EOF):
            start = self._cur()
            name_tok = self._expect(_TT.IDENTIFIER, "as generic parameter name")
            bounds: list[TypeNode] = []
            if self._match(_TT.COLON):
                bounds.append(self._type_expr())
                while self._match(_TT.PLUS):
                    bounds.append(self._type_expr())
            params.append(GenericParam(
                span=self._span_from(start),
                name=name_tok.value,
                bounds=bounds,
            ))
            if not self._match(_TT.COMMA):
                break
        self._expect(_TT.GT, "to close generic parameter list")
        return params

    def _generic_args(self) -> list[TypeNode]:
        """Parse <T, U> when used as generic arguments in a type expression."""
        if not self._match(_TT.LT):
            return []
        args: list[TypeNode] = []
        while not self._at(_TT.GT, _TT.EOF):
            args.append(self._type_expr())
            if not self._match(_TT.COMMA):
                break
        self._expect(_TT.GT, "to close generic arguments")
        return args

    # ===================================================================
    # Type expressions
    # ===================================================================

    def _type_expr(self) -> TypeNode:
        start = self._cur()

        # Array type  [T]
        if self._match(_TT.LBRACKET):
            elem = self._type_expr()
            self._expect(_TT.RBRACKET, "to close array type")
            return ArrayType(span=self._span_from(start), element_type=elem)

        # fn as a type (function type)
        if self._at(_TT.FN):
            self._advance()
            return SimpleType(span=self._span_from(start), name="fn")

        # Named type (simple or generic)
        name_tok = self._cur()
        if name_tok.type not in (_TT.IDENTIFIER, _TT.TYPE_IDENT):
            self._error(f"Expected type name, got {_tok_name(name_tok.type)}")
            return SimpleType(span=self._span(start), name="<error>")
        self._advance()
        name = name_tok.value

        # Check for generic args  <...>
        if self._at(_TT.LT):
            gen_args = self._generic_args()
            if gen_args:
                return GenericType(
                    span=self._span_from(start),
                    name=name,
                    type_params=gen_args,
                )

        # Optional sugar: T?  ->  OptionType
        # (No QUESTION token in the lexer, so skip for now.)

        return SimpleType(span=self._span_from(start), name=name)

    # ===================================================================
    # Statements
    # ===================================================================

    def _statement(self) -> Statement | None:
        if self._at(_TT.LET):
            return self._let_stmt()
        if self._at(_TT.RETURN):
            return self._return_stmt()
        if self._at(_TT.IF):
            return self._if_stmt()
        if self._at(_TT.FOR):
            return self._for_stmt()
        if self._at(_TT.WHILE):
            return self._while_stmt()
        if self._at(_TT.BREAK):
            start = self._advance()
            return BreakStmt(span=self._span(start))
        if self._at(_TT.CONTINUE):
            start = self._advance()
            return ContinueStmt(span=self._span(start))
        if self._at(_TT.SPAWN):
            return self._spawn_stmt()
        if self._at(_TT.SELECT):
            return self._select_stmt()
        if self._at(_TT.TRY):
            return self._try_catch_stmt()
        if self._at(_TT.THROW):
            return self._throw_stmt()
        return self._expr_or_assign_stmt()

    # -------------------------------------------------------------------
    # let_stmt
    # -------------------------------------------------------------------

    def _let_stmt(self) -> LetStmt:
        start = self._advance()  # consume 'let'
        is_mut = bool(self._match(_TT.MUT))
        name_tok = self._expect(_TT.IDENTIFIER, "as variable name")
        type_ann: TypeNode | None = None
        if self._match(_TT.COLON):
            type_ann = self._type_expr()
        value: Expression | None = None
        if self._match(_TT.EQ):
            value = self._expression()
        return LetStmt(
            span=self._span_from(start),
            name=name_tok.value,
            type_annotation=type_ann,
            value=value,
            mutable=is_mut,
        )

    # -------------------------------------------------------------------
    # return_stmt
    # -------------------------------------------------------------------

    def _return_stmt(self) -> ReturnStmt:
        start = self._advance()  # consume 'return'
        value: Expression | None = None
        if not self._at(_TT.RBRACE, _TT.EOF) and self._cur().type not in _SYNC_TOKENS:
            value = self._expression()
        return ReturnStmt(span=self._span_from(start), value=value)

    # -------------------------------------------------------------------
    # if_stmt
    # -------------------------------------------------------------------

    def _if_stmt(self) -> IfStmt:
        start = self._advance()  # consume 'if'
        cond = self._expression()
        body = self._block()
        elifs: list[ElifClause] = []
        while self._match(_TT.ELIF):
            elif_start = self._peek(-1)
            elif_cond = self._expression()
            elif_body = self._block()
            elifs.append(ElifClause(
                span=self._span_from(elif_start),
                condition=elif_cond,
                body=elif_body,
            ))
        else_body: BlockStmt | None = None
        if self._match(_TT.ELSE):
            else_body = self._block()
        return IfStmt(
            span=self._span_from(start),
            condition=cond,
            body=body,
            elif_clauses=elifs,
            else_body=else_body,
        )

    # -------------------------------------------------------------------
    # for_stmt
    # -------------------------------------------------------------------

    def _for_stmt(self) -> ForStmt:
        start = self._advance()  # consume 'for'
        var_tok = self._expect(_TT.IDENTIFIER, "as loop variable")
        self._expect(_TT.IN, "after loop variable")
        iterable = self._expression()
        body = self._block()
        return ForStmt(
            span=self._span_from(start),
            variable=var_tok.value,
            iterable=iterable,
            body=body,
        )

    # -------------------------------------------------------------------
    # while_stmt
    # -------------------------------------------------------------------

    def _while_stmt(self) -> WhileStmt:
        start = self._advance()  # consume 'while'
        cond = self._expression()
        body = self._block()
        return WhileStmt(
            span=self._span_from(start),
            condition=cond,
            body=body,
        )

    # -------------------------------------------------------------------
    # spawn_stmt
    # -------------------------------------------------------------------

    def _spawn_stmt(self) -> SpawnStmt:
        start = self._advance()  # consume 'spawn'
        body: BlockStmt | None = None
        target: Expression | None = None
        if self._at(_TT.LBRACE):
            body = self._block()
        else:
            target = self._expression()
        return SpawnStmt(
            span=self._span_from(start),
            target=target,
            body=body,
        )

    # -------------------------------------------------------------------
    # select_stmt
    # -------------------------------------------------------------------

    def _select_stmt(self) -> SelectStmt:
        start = self._advance()  # consume 'select'
        self._expect(_TT.LBRACE)
        branches: list[SelectBranch] = []
        while not self._at(_TT.RBRACE) and not self._at(_TT.EOF):
            ch_expr = self._expression()
            self._expect(_TT.FAT_ARROW)
            body = self._block()
            branches.append(SelectBranch(channel=ch_expr, body=body, span=self._span_from(start)))
        self._expect(_TT.RBRACE)
        return SelectStmt(span=self._span_from(start), branches=branches)

    # -------------------------------------------------------------------
    # try_catch_stmt
    # -------------------------------------------------------------------

    def _try_catch_stmt(self) -> TryCatchStmt:
        """Parse: try { body } catch (name) { handler }"""
        start = self._advance()  # consume 'try'
        try_body = self._block()

        self._expect(_TT.CATCH, "after try block")

        # Optional: catch with error variable  --  catch (e)
        catch_name = "error"  # default name if no parens
        if self._match(_TT.LPAREN):
            name_tok = self._expect(_TT.IDENTIFIER, "as error variable name")
            catch_name = name_tok.value
            self._expect(_TT.RPAREN, "after catch variable")

        catch_body = self._block()

        return TryCatchStmt(
            span=self._span_from(start),
            try_body=try_body,
            catch_name=catch_name,
            catch_body=catch_body,
        )

    # -------------------------------------------------------------------
    # throw_stmt
    # -------------------------------------------------------------------

    def _throw_stmt(self) -> ThrowStmt:
        """Parse: throw expression"""
        start = self._advance()  # consume 'throw'
        value = self._expression()
        return ThrowStmt(span=self._span_from(start), value=value)

    # -------------------------------------------------------------------
    # expr_stmt / assign_stmt
    # -------------------------------------------------------------------

    def _expr_or_assign_stmt(self) -> Statement:
        """Parse an expression; if followed by an assignment operator, produce
        an AssignStmt; otherwise wrap in ExprStmt."""
        start = self._cur()
        expr = self._expression()
        if self._cur().type in _ASSIGN_OPS:
            op_tok = self._advance()
            value = self._expression()
            return AssignStmt(
                span=self._span_from(start),
                target=expr,
                operator=op_tok.value,
                value=value,
            )
        return ExprStmt(span=self._span_from(start), expression=expr)

    # -------------------------------------------------------------------
    # block  { ... }
    # -------------------------------------------------------------------

    def _block(self) -> BlockStmt:
        start = self._cur()
        open_tok = self._expect(_TT.LBRACE, "to open block")
        stmts: list[Statement] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            try:
                node = self._block_item()
                if node is not None:
                    stmts.append(node)
            except ParseError:
                self._synchronize()
        self._expect(
            _TT.RBRACE,
            f"to close block started at line {open_tok.line}",
        )
        return BlockStmt(span=self._span_from(start), statements=stmts)

    def _block_item(self) -> Statement | None:
        """Inside a block we allow declarations (fn, struct, ...) as well as
        regular statements.  Declarations are wrapped in ExprStmt when needed."""
        attrs = self._parse_attributes()

        if self._at(_TT.FN):
            fn = self._fn_decl(attrs)
            # Wrap declaration as statement (duck-typed into Statement list).
            return fn  # type: ignore[return-value]
        if self._at(_TT.STRUCT):
            return self._struct_decl(attrs)  # type: ignore[return-value]
        if self._at(_TT.ENUM):
            return self._enum_decl(attrs)  # type: ignore[return-value]

        if attrs:
            self._error("Attributes must be followed by a declaration")

        return self._statement()

    # ===================================================================
    # Expressions  (Pratt / precedence climbing)
    # ===================================================================

    def _expression(self) -> Expression:
        return self._pipe()

    # -------------------------------------------------------------------
    # pipe:  logical_or ("|>" logical_or)*
    # -------------------------------------------------------------------

    def _pipe(self) -> Expression:
        left = self._logical_or()
        # |>  may be two tokens (PIPE GT) or a single FAT_ARROW variant.
        while self._at(_TT.PIPE) and self._at_ahead(1, _TT.GT):
            self._advance()  # PIPE
            self._advance()  # GT
            right = self._logical_or()
            left = PipeExpr(span=self._span_from(self._peek(-1)), left=left, right=right)
        return left

    # -------------------------------------------------------------------
    # logical_or:  logical_and ("or" logical_and)*
    # -------------------------------------------------------------------

    def _logical_or(self) -> Expression:
        left = self._logical_and()
        while self._match(_TT.OR):
            right = self._logical_and()
            left = BinaryOp(
                span=self._span_from(self._peek(-1)),
                left=left, operator="or", right=right,
            )
        return left

    # -------------------------------------------------------------------
    # logical_and:  equality ("and" equality)*
    # -------------------------------------------------------------------

    def _logical_and(self) -> Expression:
        left = self._equality()
        while self._match(_TT.AND):
            right = self._equality()
            left = BinaryOp(
                span=self._span_from(self._peek(-1)),
                left=left, operator="and", right=right,
            )
        return left

    # -------------------------------------------------------------------
    # equality:  comparison (("==" | "!=") comparison)*
    # -------------------------------------------------------------------

    def _equality(self) -> Expression:
        left = self._comparison()
        while self._at(_TT.EQ_EQ, _TT.BANG_EQ):
            op_tok = self._advance()
            right = self._comparison()
            left = BinaryOp(
                span=self._span_from(op_tok),
                left=left, operator=op_tok.value, right=right,
            )
        return left

    # -------------------------------------------------------------------
    # comparison:  range (("<" | ">" | "<=" | ">=") range)*
    # -------------------------------------------------------------------

    def _comparison(self) -> Expression:
        left = self._range()
        while self._at(_TT.LT, _TT.GT, _TT.LT_EQ, _TT.GT_EQ):
            op_tok = self._advance()
            right = self._range()
            left = BinaryOp(
                span=self._span_from(op_tok),
                left=left, operator=op_tok.value, right=right,
            )
        return left

    # -------------------------------------------------------------------
    # range:  addition ((".." | "..=") addition)?
    # -------------------------------------------------------------------

    def _range(self) -> Expression:
        left = self._addition()
        if self._at(_TT.DOT_DOT, _TT.DOT_DOT_EQ):
            inclusive = self._cur().type == _TT.DOT_DOT_EQ
            self._advance()
            right = self._addition()
            return RangeExpr(
                span=self._span_from(self._peek(-1)),
                start=left, end=right, inclusive=inclusive,
            )
        return left

    # -------------------------------------------------------------------
    # addition:  multiplication (("+" | "-") multiplication)*
    # -------------------------------------------------------------------

    def _addition(self) -> Expression:
        left = self._multiplication()
        while self._at(_TT.PLUS, _TT.MINUS):
            op_tok = self._advance()
            right = self._multiplication()
            left = BinaryOp(
                span=self._span_from(op_tok),
                left=left, operator=op_tok.value, right=right,
            )
        return left

    # -------------------------------------------------------------------
    # multiplication:  matrix_mul (("*" | "/" | "%") matrix_mul)*
    # -------------------------------------------------------------------

    def _multiplication(self) -> Expression:
        left = self._matrix_mul()
        while self._at(_TT.STAR, _TT.SLASH, _TT.PERCENT):
            op_tok = self._advance()
            right = self._matrix_mul()
            left = BinaryOp(
                span=self._span_from(op_tok),
                left=left, operator=op_tok.value, right=right,
            )
        return left

    # -------------------------------------------------------------------
    # matrix_mul:  power ("@" power)*
    # -------------------------------------------------------------------

    def _matrix_mul(self) -> Expression:
        left = self._power()
        while self._at(_TT.AT):
            op_tok = self._advance()
            right = self._power()
            left = BinaryOp(
                span=self._span_from(op_tok),
                left=left, operator="@", right=right,
            )
        return left

    # -------------------------------------------------------------------
    # power:  unary ("**" unary)*   (right-associative)
    # -------------------------------------------------------------------

    def _power(self) -> Expression:
        base = self._unary()
        if self._match(_TT.POWER):
            exp = self._power()  # right-associative via recursion
            return BinaryOp(
                span=self._span_from(self._peek(-1)),
                left=base, operator="**", right=exp,
            )
        return base

    # -------------------------------------------------------------------
    # unary:  ("-" | "not" | "~") unary  |  postfix
    # -------------------------------------------------------------------

    def _unary(self) -> Expression:
        if self._at(_TT.MINUS, _TT.NOT, _TT.TILDE):
            op_tok = self._advance()
            operand = self._unary()
            return UnaryOp(
                span=self._span_from(op_tok),
                operator=op_tok.value,
                operand=operand,
            )
        return self._postfix()

    # -------------------------------------------------------------------
    # postfix:  primary (call | index | field_access | method_call)*
    # -------------------------------------------------------------------

    def _postfix(self) -> Expression:
        node = self._primary()

        while True:
            if self._at(_TT.LPAREN):
                # Function call
                self._advance()
                args = self._expr_list(_TT.RPAREN)
                self._expect(_TT.RPAREN, "to close function call")
                node = FnCall(
                    span=self._span_from(self._peek(-1)),
                    callee=node,
                    args=args,
                )
            elif self._at(_TT.LBRACKET):
                # Index access
                self._advance()
                index = self._expression()
                self._expect(_TT.RBRACKET, "to close index expression")
                node = IndexAccess(
                    span=self._span_from(self._peek(-1)),
                    object=node,
                    index=index,
                )
            elif self._at(_TT.DOT):
                self._advance()
                field_tok = self._expect(_TT.IDENTIFIER, "as field/method name")
                if self._at(_TT.LPAREN):
                    # Method call
                    self._advance()
                    args = self._expr_list(_TT.RPAREN)
                    self._expect(_TT.RPAREN, "to close method call")
                    node = MethodCall(
                        span=self._span_from(self._peek(-1)),
                        object=node,
                        method=field_tok.value,
                        args=args,
                    )
                else:
                    # Field access
                    node = FieldAccess(
                        span=self._span_from(self._peek(-1)),
                        object=node,
                        field=field_tok.value,
                    )
            else:
                break

        return node

    # -------------------------------------------------------------------
    # primary
    # -------------------------------------------------------------------

    def _primary(self) -> Expression:
        tok = self._cur()

        # Integer literal
        if tok.type == _TT.INTEGER:
            self._advance()
            return IntLiteral(span=self._span(tok), value=int(tok.value, 0))

        # Float literal
        if tok.type == _TT.FLOAT:
            self._advance()
            return FloatLiteral(span=self._span(tok), value=float(tok.value))

        # String literal
        if tok.type == _TT.STRING:
            self._advance()
            return StringLiteral(span=self._span(tok), value=tok.value)

        # Char literal
        if tok.type == _TT.CHAR:
            self._advance()
            return CharLiteral(span=self._span(tok), value=tok.value)

        # Bool literals
        if tok.type == _TT.TRUE:
            self._advance()
            return BoolLiteral(span=self._span(tok), value=True)
        if tok.type == _TT.FALSE:
            self._advance()
            return BoolLiteral(span=self._span(tok), value=False)

        # None literal
        if tok.type == _TT.NONE:
            self._advance()
            return NoneLiteral(span=self._span(tok))

        # Identifier (possibly followed by struct literal)
        if tok.type in (_TT.IDENTIFIER, _TT.TYPE_IDENT):
            self._advance()
            if self._at(_TT.LBRACE) and self._is_struct_literal():
                return self._struct_literal(tok)
            return Identifier(span=self._span(tok), name=tok.value)

        # Parenthesised expression
        if tok.type == _TT.LPAREN:
            self._advance()
            expr = self._expression()
            self._expect(_TT.RPAREN, f"to close '(' at line {tok.line}")
            return expr  # no wrapper needed; grouping is implicit

        # Array literal  [a, b, c]
        if tok.type == _TT.LBRACKET:
            self._advance()
            elements: list[Expression] = []
            if not self._at(_TT.RBRACKET):
                elements = self._expr_list(_TT.RBRACKET)
            self._expect(_TT.RBRACKET, f"to close '[' at line {tok.line}")
            return ArrayLiteral(span=self._span_from(tok), elements=elements)

        # Map literal  {"key": value, ...}  or  {}
        if tok.type == _TT.LBRACE and self._is_map_literal():
            return self._map_literal()

        # Block expression  { ... }
        if tok.type == _TT.LBRACE:
            blk = self._block()
            return blk  # type: ignore[return-value]

        # If expression
        if tok.type == _TT.IF:
            return self._if_expr()

        # Match expression
        if tok.type == _TT.MATCH:
            return self._match_expr()

        # Anonymous function   fn(params) { body }  or  fn(params) -> T { body }
        if tok.type == _TT.FN and self._at_ahead(1, _TT.LPAREN):
            return self._anonymous_fn()

        # Lambda   |params| -> body   or   |params| { block }
        if tok.type == _TT.PIPE:
            return self._lambda_expr()

        # Quantum block
        if tok.type == _TT.QUANTUM:
            self._advance()
            body_block = self._block()
            return QuantumBlock(
                span=self._span_from(tok),
                body=body_block.statements,
            )

        # Comptime block
        if tok.type == _TT.COMPTIME:
            self._advance()
            body_block = self._block()
            return ComptimeBlock(
                span=self._span_from(tok),
                body=body_block.statements,
            )

        # Channel keywords used as function calls: chan(), send(), recv(), ask()
        if tok.type in (_TT.SEND, _TT.RECV, _TT.ASK, _TT.CHAN):
            self._advance()
            return Identifier(span=self._span(tok), name=tok.value)

        # Nothing matched -- error
        self._error(f"Unexpected token {_tok_name(tok.type)} ({tok.value!r})")
        raise ParseError(
            f"Unexpected token {_tok_name(tok.type)}",
            tok.file, tok.line, tok.column,
        )

    # -------------------------------------------------------------------
    # Struct literal lookahead
    # -------------------------------------------------------------------

    def _is_struct_literal(self) -> bool:
        """Heuristic: does '{' start a struct literal (Name { field: ... })?"""
        if (self._at_ahead(0, _TT.LBRACE)
                and self._at_ahead(1, _TT.IDENTIFIER)
                and self._at_ahead(2, _TT.COLON)):
            return True
        # Empty struct literal: Name { }
        if self._at_ahead(0, _TT.LBRACE) and self._at_ahead(1, _TT.RBRACE):
            return True
        return False

    def _struct_literal(self, name_tok: Token) -> StructLiteral:
        self._advance()  # consume '{'
        fields: list[Tuple[str, Expression]] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            field_tok = self._expect(_TT.IDENTIFIER, "as struct field name")
            self._expect(_TT.COLON, "after struct field name")
            value = self._expression()
            fields.append((field_tok.value, value))
            if not self._match(_TT.COMMA):
                break
        self._expect(
            _TT.RBRACE,
            f"to close struct literal started at line {name_tok.line}",
        )
        return StructLiteral(
            span=self._span_from(name_tok),
            type_name=name_tok.value,
            field_values=fields,
        )

    # -------------------------------------------------------------------
    # Map literal lookahead
    # -------------------------------------------------------------------

    def _is_map_literal(self) -> bool:
        """Heuristic: does '{' start a map literal?

        A map literal is ``{"key": value, ...}``.  We recognise it when the
        token immediately after ``{`` is a STRING followed by ``:``.  An
        empty ``{}`` is also treated as an empty map (not an empty block)
        when it appears in expression position that already reached here.
        """
        # { STRING : ...  ->  definitely a map literal
        if (self._at_ahead(0, _TT.LBRACE)
                and self._at_ahead(1, _TT.STRING)
                and self._at_ahead(2, _TT.COLON)):
            return True
        # {} -> treat as empty map literal
        if (self._at_ahead(0, _TT.LBRACE)
                and self._at_ahead(1, _TT.RBRACE)):
            return True
        return False

    def _map_literal(self) -> MapLiteral:
        """Parse ``{"key": value, ...}`` map literal.  Supports trailing commas."""
        start = self._advance()  # consume '{'
        pairs: list[Tuple[Expression, Expression]] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            key = self._expression()
            self._expect(_TT.COLON, "after map key")
            value = self._expression()
            pairs.append((key, value))
            if not self._match(_TT.COMMA):
                break
        self._expect(
            _TT.RBRACE,
            f"to close map literal started at line {start.line}",
        )
        return MapLiteral(span=self._span_from(start), pairs=pairs)

    # -------------------------------------------------------------------
    # If expression
    # -------------------------------------------------------------------

    def _if_expr(self) -> IfExpr:
        start = self._advance()  # consume 'if'
        cond = self._expression()
        then_expr = self._expression()
        else_expr: Expression | None = None
        if self._match(_TT.ELSE):
            else_expr = self._expression()
        return IfExpr(
            span=self._span_from(start),
            condition=cond,
            then_expr=then_expr,
            else_expr=else_expr,
        )

    # -------------------------------------------------------------------
    # Match expression   match expr { pattern => body, ... }
    # -------------------------------------------------------------------

    def _match_expr(self) -> MatchExpr:
        """Parse: match expr { pattern => body, pattern => body, ... }"""
        start = self._advance()  # consume 'match'
        subject = self._expression()
        self._expect(_TT.LBRACE, "after match expression")

        arms: list[MatchArm] = []
        while not self._at(_TT.RBRACE, _TT.EOF):
            arm_start = self._cur()

            # Parse pattern
            pattern: Expression | None = None
            if (self._at(_TT.IDENTIFIER) and self._cur().value == "_"
                    and self._at_ahead(1, _TT.FAT_ARROW)):
                # Wildcard: _
                self._advance()  # consume '_'
                pattern = None
            else:
                # Parse pattern as expression (literal, enum path, variable)
                pattern = self._match_pattern()

            self._expect(_TT.FAT_ARROW, "after match pattern")

            # Parse body -- either a block { ... } or a single expression
            body: Expression | BlockStmt
            if self._at(_TT.LBRACE):
                body = self._block()
            else:
                body = self._expression()

            arms.append(MatchArm(
                span=self._span_from(arm_start),
                pattern=pattern,
                body=body,  # type: ignore[arg-type]
            ))

            # Optional comma between arms
            self._match(_TT.COMMA)

        self._expect(
            _TT.RBRACE,
            f"to close match started at line {start.line}",
        )
        return MatchExpr(
            span=self._span_from(start),
            value=subject,
            arms=arms,
        )

    def _match_pattern(self) -> Expression:
        """Parse a match pattern: literal, Enum::Variant, or identifier."""
        tok = self._cur()

        # Handle Enum::Variant pattern: IDENTIFIER :: IDENTIFIER
        if (tok.type == _TT.IDENTIFIER
                and self._at_ahead(1, _TT.COLON_COLON)
                and self._at_ahead(2, _TT.IDENTIFIER)):
            enum_tok = self._advance()  # enum name
            self._advance()  # ::
            variant_tok = self._advance()  # variant name
            # Return as an Identifier with the full path, since the interpreter
            # stores enum variants as "EnumName::VariantName" in the environment
            return Identifier(
                span=self._span_from(enum_tok),
                name=f"{enum_tok.value}::{variant_tok.value}",
            )

        # Fall back to normal expression parsing for literals, identifiers, etc.
        return self._expression()

    # -------------------------------------------------------------------
    # Anonymous fn   fn(params) { body }   or   fn(params) -> T { body }
    # -------------------------------------------------------------------

    def _anonymous_fn(self) -> Lambda:
        """Parse ``fn(params) { body }`` as an anonymous function expression."""
        start = self._advance()  # consume 'fn'
        self._expect(_TT.LPAREN, "to open parameter list")
        params = self._param_list()
        self._expect(_TT.RPAREN, "to close parameter list")

        ret_type: TypeNode | None = None
        if self._match(_TT.ARROW):
            ret_type = self._type_expr()

        body = self._block()

        return Lambda(
            span=self._span_from(start),
            params=params,
            return_type=ret_type,
            body=body,
        )

    # -------------------------------------------------------------------
    # Lambda   |x, y| expr   or   |x, y| { block }
    # -------------------------------------------------------------------

    def _lambda_expr(self) -> Lambda:
        start = self._advance()  # consume first '|'
        params: list[Parameter] = []
        if not self._at(_TT.PIPE):
            params = self._param_list_until_pipe()
        self._expect(_TT.PIPE, "to close lambda parameter list")

        ret_type: TypeNode | None = None
        if self._match(_TT.ARROW):
            ret_type = self._type_expr()

        body: Expression | BlockStmt
        if self._at(_TT.LBRACE):
            body = self._block()
        else:
            body = self._expression()

        return Lambda(
            span=self._span_from(start),
            params=params,
            return_type=ret_type,
            body=body,
        )

    def _param_list_until_pipe(self) -> list[Parameter]:
        params: list[Parameter] = []
        while not self._at(_TT.PIPE, _TT.EOF):
            params.append(self._parameter())
            if not self._match(_TT.COMMA):
                break
        return params

    # -------------------------------------------------------------------
    # Comma-separated expression list (shared for calls, arrays, attrs)
    # -------------------------------------------------------------------

    def _expr_list(self, end: TokenType) -> list[Expression]:
        """Parse comma-separated expressions until *end* token.
        Supports trailing commas."""
        args: list[Expression] = []
        while not self._at(end, _TT.EOF):
            args.append(self._expression())
            if not self._match(_TT.COMMA):
                break
        return args


# ═══════════════════════════════════════════════════════════════════════════
# Public API
# ═══════════════════════════════════════════════════════════════════════════

def parse(tokens: list[Token], *, raise_on_error: bool = True) -> Module:
    """Parse a list of tokens into a :class:`Module` AST node.

    Parameters
    ----------
    tokens : list[Token]
        The token stream produced by the Kryos lexer.  Must end with an
        ``EOF`` token.
    raise_on_error : bool
        If *True* (default), raises :class:`ParseErrors` when the parser
        encounters problems.  If *False*, errors are attached to the
        returned module as ``module._parse_errors`` and parsing continues
        as far as possible.

    Returns
    -------
    Module
        The root AST node representing the entire program.
    """
    parser = Parser(tokens)
    module = parser.parse_module()

    if parser.errors:
        if raise_on_error:
            raise ParseErrors(parser.errors)
        module._parse_errors = parser.errors  # type: ignore[attr-defined]

    return module
