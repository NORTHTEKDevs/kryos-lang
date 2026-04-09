//! Core parser implementation: recursive descent for declarations/statements,
//! Pratt (top-down operator precedence) for expressions.

use kryos_ast::*;
use kryos_errors::{Diagnostic, Span};
use kryos_lexer::{Token, TokenKind};

// ---------------------------------------------------------------------------
// Operator precedence levels for Pratt parsing
// ---------------------------------------------------------------------------

/// Binding power for binary/postfix operators.
/// Returns `(left_bp, right_bp)`. A higher number means tighter binding.
/// Right-associative operators have `right_bp < left_bp + 1`.
fn infix_binding_power(kind: TokenKind, next: Option<TokenKind>) -> Option<(u8, u8)> {
    let bp = match kind {
        // 1. Pipe  |>  — lowest binary
        TokenKind::Pipe if next == Some(TokenKind::Gt) => (2, 3),
        // 2. or
        TokenKind::Or => (4, 5),
        // 3. and
        TokenKind::And => (6, 7),
        // 4. == != < > <= >=
        TokenKind::EqEq | TokenKind::BangEq |
        TokenKind::Lt | TokenKind::Gt |
        TokenKind::LtEq | TokenKind::GtEq => (8, 9),
        // 5. |  (bitwise or)
        TokenKind::Pipe => (10, 11),
        // 6. ^
        TokenKind::Caret => (12, 13),
        // 7. &
        TokenKind::Amp => (14, 15),
        // 8. << >>
        TokenKind::Shl | TokenKind::Shr => (16, 17),
        // 9. + -
        TokenKind::Plus | TokenKind::Minus => (18, 19),
        // 10. * / %
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => (20, 21),
        // 11. ** (right-assoc: right_bp == left_bp)
        TokenKind::Power => (23, 22),
        // 13. `as` cast
        TokenKind::As => (25, 26),
        // Range operators: very low (below pipe)
        TokenKind::DotDot | TokenKind::DotDotEq => (1, 1),
        _ => return None,
    };
    Some(bp)
}

/// Prefix binding power for unary operators.
fn prefix_binding_power(kind: TokenKind) -> Option<u8> {
    match kind {
        // 12. unary - not ~ *
        TokenKind::Minus | TokenKind::Not | TokenKind::Tilde | TokenKind::Star => Some(24),
        // & (borrow / address-of) — same precedence as other unary prefix ops
        TokenKind::Amp => Some(24),
        // shared / move / weak — prefix keyword operators (very low, just above range)
        TokenKind::Shared | TokenKind::Move | TokenKind::Weak => Some(3),
        _ => None,
    }
}

/// Postfix binding power (field access, index, call).
const POSTFIX_BP: u8 = 28;

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    /// When `true`, suppress parsing `Name { ... }` as a struct literal.
    /// Set while parsing conditions for `if`, `while`, `elif`, `for`, `match`
    /// so that `{` is treated as a block opener, not a struct-literal opener.
    no_struct_literal: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
            no_struct_literal: false,
        }
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    // -----------------------------------------------------------------------
    // Token navigation
    // -----------------------------------------------------------------------

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> TokenKind {
        self.peek().kind
    }

    fn peek_nth(&self, n: usize) -> TokenKind {
        let idx = (self.pos + n).min(self.tokens.len() - 1);
        self.tokens[idx].kind
    }

    fn at_end(&self) -> bool {
        self.peek_kind() == TokenKind::Eof
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if !self.at_end() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Token {
        if self.check(kind) {
            self.advance().clone()
        } else {
            let span = self.peek().span;
            self.error(format!("expected {:?}, found {:?}", kind, self.peek_kind()), span);
            // Return a dummy token so callers can keep going.
            Token::new(kind, span, "")
        }
    }

    fn expect_ident(&mut self) -> (String, Span) {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Ident | TokenKind::TypeIdent => {
                self.advance();
                (tok.text.clone(), tok.span)
            }
            // Allow keywords used as identifiers in certain positions
            _ if tok.kind.is_keyword() => {
                self.advance();
                (tok.text.clone(), tok.span)
            }
            _ => {
                let span = tok.span;
                self.error(format!("expected identifier, found {:?}", tok.kind), span);
                ("<error>".to_string(), span)
            }
        }
    }

    fn expect_name(&mut self) -> (String, Span) {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Ident | TokenKind::TypeIdent => {
                self.advance();
                (tok.text.clone(), tok.span)
            }
            _ => {
                let span = tok.span;
                self.error(format!("expected name, found {:?}", tok.kind), span);
                ("<error>".to_string(), span)
            }
        }
    }

    fn error(&mut self, message: String, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(message).with_label(span, "here"),
        );
    }

    /// Synchronize after a parse error — skip tokens until we reach a
    /// likely declaration or statement boundary.
    fn synchronize(&mut self) {
        while !self.at_end() {
            match self.peek_kind() {
                TokenKind::Fn | TokenKind::Struct | TokenKind::Enum |
                TokenKind::Trait | TokenKind::Impl | TokenKind::Actor |
                TokenKind::Type | TokenKind::Use | TokenKind::Extern |
                TokenKind::Pub | TokenKind::At |
                TokenKind::Let | TokenKind::Return | TokenKind::If |
                TokenKind::For | TokenKind::While | TokenKind::Break |
                TokenKind::Continue | TokenKind::Spawn | TokenKind::Select |
                TokenKind::Try | TokenKind::Throw |
                TokenKind::RBrace => return,
                _ => { self.advance(); }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Module (top-level)
    // -----------------------------------------------------------------------

    pub fn parse_module(&mut self) -> Module {
        let start_span = self.peek().span;
        let mut declarations = Vec::new();

        while !self.at_end() {
            match self.parse_declaration() {
                Some(decl) => declarations.push(decl),
                None => {
                    if !self.at_end() {
                        let span = self.peek().span;
                        self.error(format!("unexpected token {:?}", self.peek_kind()), span);
                        self.synchronize();
                    }
                }
            }
        }

        let end_span = if declarations.is_empty() { start_span } else { declarations.last().unwrap().span() };
        Module {
            name: String::new(),
            declarations,
            span: start_span.merge(end_span),
        }
    }

    // -----------------------------------------------------------------------
    // Declarations
    // -----------------------------------------------------------------------

    fn collect_doc_comments(&mut self) -> Vec<String> {
        let mut docs = Vec::new();
        while self.check(TokenKind::DocComment) {
            let tok = self.advance().clone();
            docs.push(tok.text.clone());
        }
        docs
    }

    fn parse_declaration(&mut self) -> Option<Decl> {
        // Collect doc comments before annotations/modifiers
        let doc_comments = self.collect_doc_comments();

        // Collect annotations
        let mut annotations = Vec::new();
        while self.check(TokenKind::At) {
            annotations.push(self.parse_annotation());
        }

        let public = self.eat(TokenKind::Pub);

        match self.peek_kind() {
            TokenKind::Fn => Some(self.parse_fn_decl(public, annotations, doc_comments)),
            TokenKind::Struct => Some(self.parse_struct_decl(public, annotations, doc_comments)),
            TokenKind::Enum => Some(self.parse_enum_decl(public, annotations, doc_comments)),
            TokenKind::Trait => Some(self.parse_trait_decl(public, doc_comments)),
            TokenKind::Impl => Some(self.parse_impl_decl(doc_comments)),
            TokenKind::Actor => Some(self.parse_actor_decl(annotations)),
            TokenKind::Type => Some(self.parse_type_alias(public)),
            TokenKind::Use => Some(self.parse_import()),
            TokenKind::Extern => Some(self.parse_extern()),
            TokenKind::Let => Some(self.parse_const_decl(public, doc_comments)),
            _ => {
                if !annotations.is_empty() || public {
                    let span = self.peek().span;
                    self.error("expected declaration after modifier".to_string(), span);
                    self.synchronize();
                }
                None
            }
        }
    }

    fn parse_annotation(&mut self) -> Annotation {
        let at_tok = self.expect(TokenKind::At);
        let start = at_tok.span;
        let (name, _) = self.expect_ident();
        let mut args = Vec::new();
        let mut end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;

        if self.eat(TokenKind::LParen) {
            while !self.check(TokenKind::RParen) && !self.at_end() {
                let tok = self.advance().clone();
                args.push(tok.text.clone());
                if !self.check(TokenKind::RParen) {
                    self.eat(TokenKind::Comma);
                }
            }
            let rparen = self.expect(TokenKind::RParen);
            end = rparen.span;
        }

        Annotation { name, args, span: start.merge(end) }
    }

    fn parse_generics(&mut self) -> Vec<GenericParam> {
        let mut generics = Vec::new();
        if !self.eat(TokenKind::Lt) {
            return generics;
        }
        while !self.check(TokenKind::Gt) && !self.at_end() {
            let (name, span_start) = self.expect_name();
            let mut bounds = Vec::new();
            if self.eat(TokenKind::Colon) {
                // Parse bounds: `T: Bound1 + Bound2`
                let (b, _) = self.expect_name();
                bounds.push(b);
                while self.eat(TokenKind::Plus) {
                    let (b, _) = self.expect_name();
                    bounds.push(b);
                }
            }
            let span_end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
            generics.push(GenericParam { name, bounds, span: span_start.merge(span_end) });
            if !self.check(TokenKind::Gt) {
                self.expect(TokenKind::Comma);
            }
        }
        self.expect(TokenKind::Gt);
        generics
    }

    fn parse_fn_decl(&mut self, public: bool, annotations: Vec<Annotation>, doc_comments: Vec<String>) -> Decl {
        let fn_tok = self.expect(TokenKind::Fn);
        let start = fn_tok.span;
        let (name, _) = self.expect_name();
        let generics = self.parse_generics();
        self.expect(TokenKind::LParen);
        let params = self.parse_param_list();
        self.expect(TokenKind::RParen);

        let ret_ty = if self.eat(TokenKind::Arrow) {
            Some(self.parse_type())
        } else {
            None
        };

        let body = if self.check(TokenKind::LBrace) {
            Some(self.parse_block())
        } else {
            // Trait method signature — no body
            self.eat(TokenKind::Semicolon);
            None
        };

        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Decl::Function {
            name,
            generics,
            params,
            ret_ty,
            body,
            public,
            annotations,
            doc_comments,
            span: start.merge(end),
        }
    }

    fn parse_param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        while !self.check(TokenKind::RParen) && !self.at_end() {
            let before = self.pos;
            // Handle `self` parameter
            if self.peek().text == "self" {
                let tok = self.advance().clone();
                let ty = if self.eat(TokenKind::Colon) {
                    Some(self.parse_type())
                } else {
                    None
                };
                let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
                params.push(Param {
                    name: "self".to_string(),
                    ty,
                    default: None,
                    span: tok.span.merge(end),
                });
            } else {
                let (name, name_span) = self.expect_name();
                let ty = if self.eat(TokenKind::Colon) {
                    Some(self.parse_type())
                } else {
                    None
                };
                let default = if self.eat(TokenKind::Eq) {
                    Some(Box::new(self.parse_expr()))
                } else {
                    None
                };
                let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
                params.push(Param {
                    name,
                    ty,
                    default,
                    span: name_span.merge(end),
                });
            }
            if !self.check(TokenKind::RParen) {
                self.expect(TokenKind::Comma);
            }
            // Guard against infinite loops: if no progress was made, skip the
            // offending token so we eventually reach RParen or Eof.
            if self.pos == before {
                self.advance();
            }
        }
        params
    }

    fn parse_struct_decl(&mut self, public: bool, annotations: Vec<Annotation>, doc_comments: Vec<String>) -> Decl {
        let kw = self.expect(TokenKind::Struct);
        let start = kw.span;
        let (name, _) = self.expect_name();
        let generics = self.parse_generics();
        self.expect(TokenKind::LBrace);

        let mut fields = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let field_public = self.eat(TokenKind::Pub);
            let (fname, fspan) = self.expect_name();
            self.expect(TokenKind::Colon);
            let ty = self.parse_type();
            let default = if self.eat(TokenKind::Eq) {
                Some(self.parse_expr())
            } else {
                None
            };
            let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
            fields.push(StructField {
                name: fname,
                ty,
                public: field_public,
                default,
                span: fspan.merge(end),
            });
            if !self.check(TokenKind::RBrace) {
                self.eat(TokenKind::Comma);
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Decl::Struct {
            name,
            generics,
            fields,
            public,
            annotations,
            doc_comments,
            span: start.merge(rbrace.span),
        }
    }

    fn parse_enum_decl(&mut self, public: bool, annotations: Vec<Annotation>, doc_comments: Vec<String>) -> Decl {
        let kw = self.expect(TokenKind::Enum);
        let start = kw.span;
        let (name, _) = self.expect_name();
        let generics = self.parse_generics();
        self.expect(TokenKind::LBrace);

        let mut variants = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let (vname, vspan) = self.expect_name();
            let mut variant_fields = Vec::new();
            if self.eat(TokenKind::LParen) {
                while !self.check(TokenKind::RParen) && !self.at_end() {
                    variant_fields.push(self.parse_type());
                    if !self.check(TokenKind::RParen) {
                        self.expect(TokenKind::Comma);
                    }
                }
                self.expect(TokenKind::RParen);
            }
            let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
            variants.push(EnumVariant {
                name: vname,
                fields: variant_fields,
                span: vspan.merge(end),
            });
            if !self.check(TokenKind::RBrace) {
                self.eat(TokenKind::Comma);
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Decl::Enum {
            name,
            generics,
            variants,
            public,
            annotations,
            doc_comments,
            span: start.merge(rbrace.span),
        }
    }

    fn parse_trait_decl(&mut self, public: bool, doc_comments: Vec<String>) -> Decl {
        let kw = self.expect(TokenKind::Trait);
        let start = kw.span;
        let (name, _) = self.expect_name();
        let generics = self.parse_generics();
        self.expect(TokenKind::LBrace);

        let mut methods = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let method_docs = self.collect_doc_comments();
            methods.push(self.parse_fn_decl(false, Vec::new(), method_docs));
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Decl::Trait {
            name,
            generics,
            methods,
            public,
            doc_comments,
            span: start.merge(rbrace.span),
        }
    }

    fn parse_impl_decl(&mut self, doc_comments: Vec<String>) -> Decl {
        let kw = self.expect(TokenKind::Impl);
        let start = kw.span;

        let generics = self.parse_generics();
        let (first_name, _) = self.expect_name();

        // Check for `impl Trait for Target`
        let (target, trait_name) = if self.eat(TokenKind::For) {
            let (tgt, _) = self.expect_name();
            (tgt, Some(first_name))
        } else {
            (first_name, None)
        };

        self.expect(TokenKind::LBrace);
        let mut methods = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let method_docs = self.collect_doc_comments();
            let pub_method = self.eat(TokenKind::Pub);
            methods.push(self.parse_fn_decl(pub_method, Vec::new(), method_docs));
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Decl::Impl {
            target,
            trait_name,
            generics,
            methods,
            doc_comments,
            span: start.merge(rbrace.span),
        }
    }

    fn parse_actor_decl(&mut self, annotations: Vec<Annotation>) -> Decl {
        let kw = self.expect(TokenKind::Actor);
        let start = kw.span;
        let (name, _) = self.expect_name();
        self.expect(TokenKind::LBrace);

        let mut state_fields = Vec::new();
        let mut handlers = Vec::new();

        while !self.check(TokenKind::RBrace) && !self.at_end() {
            if self.check(TokenKind::Fn) {
                // Parse handler
                let fn_tok = self.expect(TokenKind::Fn);
                let (hname, _) = self.expect_name();
                self.expect(TokenKind::LParen);
                let params = self.parse_param_list();
                self.expect(TokenKind::RParen);
                let ret_ty = if self.eat(TokenKind::Arrow) {
                    Some(self.parse_type())
                } else {
                    None
                };
                let body = self.parse_block();
                let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
                handlers.push(MessageHandler {
                    name: hname,
                    params,
                    ret_ty,
                    body,
                    span: fn_tok.span.merge(end),
                });
            } else {
                // State field
                let (fname, fspan) = self.expect_name();
                self.expect(TokenKind::Colon);
                let ty = self.parse_type();
                let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
                state_fields.push(StructField {
                    name: fname,
                    ty,
                    public: false,
                    default: None,
                    span: fspan.merge(end),
                });
                if !self.check(TokenKind::RBrace) && !self.check(TokenKind::Fn) {
                    self.eat(TokenKind::Comma);
                }
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Decl::Actor {
            name,
            state_fields,
            handlers,
            annotations,
            span: start.merge(rbrace.span),
        }
    }

    fn parse_type_alias(&mut self, public: bool) -> Decl {
        let kw = self.expect(TokenKind::Type);
        let start = kw.span;
        let (name, _) = self.expect_name();
        let generics = self.parse_generics();
        self.expect(TokenKind::Eq);
        let ty = self.parse_type();
        let end = ty.span();
        Decl::TypeAlias {
            name,
            generics,
            ty,
            public,
            span: start.merge(end),
        }
    }

    fn parse_const_decl(&mut self, public: bool, doc_comments: Vec<String>) -> Decl {
        let kw = self.expect(TokenKind::Let);
        let start = kw.span;
        // Skip optional 'mut' (top-level let is always immutable, but accept it gracefully)
        self.eat(TokenKind::Mut);
        let (name, _) = self.expect_name();
        let ty = if self.eat(TokenKind::Colon) {
            Some(self.parse_type())
        } else {
            None
        };
        self.expect(TokenKind::Eq);
        let value = self.parse_expr();
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Decl::Const {
            name,
            ty,
            value: Box::new(value),
            public,
            doc_comments,
            span: start.merge(end),
        }
    }

    fn parse_import(&mut self) -> Decl {
        let kw = self.expect(TokenKind::Use);
        let start = kw.span;
        let mut segments = Vec::new();

        let (first, _) = self.expect_ident();
        segments.push(first);

        while self.eat(TokenKind::ColonColon) {
            // Check for grouped imports: `{Item1, Item2}`
            if self.check(TokenKind::LBrace) {
                self.advance();
                let mut items = Vec::new();
                while !self.check(TokenKind::RBrace) && !self.at_end() {
                    let (item, _) = self.expect_name();
                    items.push(item);
                    if !self.check(TokenKind::RBrace) {
                        self.expect(TokenKind::Comma);
                    }
                }
                let rbrace = self.expect(TokenKind::RBrace);
                return Decl::Import {
                    path: ImportPath {
                        segments,
                        alias: None,
                        items,
                        span: start.merge(rbrace.span),
                    },
                    span: start.merge(rbrace.span),
                };
            }
            let (seg, _) = self.expect_ident();
            segments.push(seg);
        }

        let alias = if self.eat(TokenKind::As) {
            let (a, _) = self.expect_name();
            Some(a)
        } else {
            None
        };

        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Decl::Import {
            path: ImportPath {
                segments,
                alias,
                items: Vec::new(),
                span: start.merge(end),
            },
            span: start.merge(end),
        }
    }

    fn parse_extern(&mut self) -> Decl {
        let kw = self.expect(TokenKind::Extern);
        let start = kw.span;

        let abi = if self.check(TokenKind::String) {
            let tok = self.advance().clone();
            tok.text.clone()
        } else {
            "C".to_string()
        };

        self.expect(TokenKind::LBrace);
        let mut items = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            items.push(self.parse_fn_decl(false, Vec::new(), Vec::new()));
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Decl::Extern {
            abi,
            items,
            span: start.merge(rbrace.span),
        }
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    fn parse_block(&mut self) -> Block {
        let lbrace = self.expect(TokenKind::LBrace);
        let start = lbrace.span;
        let mut stmts = Vec::new();

        while !self.check(TokenKind::RBrace) && !self.at_end() {
            match self.parse_statement() {
                Some(stmt) => stmts.push(stmt),
                None => {
                    if !self.check(TokenKind::RBrace) && !self.at_end() {
                        let span = self.peek().span;
                        self.error(format!("unexpected token {:?} in block", self.peek_kind()), span);
                        self.advance();
                    }
                }
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Block { stmts, span: start.merge(rbrace.span) }
    }

    fn parse_statement(&mut self) -> Option<Stmt> {
        match self.peek_kind() {
            TokenKind::Let => Some(self.parse_let()),
            TokenKind::Return => Some(self.parse_return()),
            TokenKind::If => Some(self.parse_if_stmt()),
            TokenKind::Parallel if self.pos + 1 < self.tokens.len()
                && self.tokens[self.pos + 1].kind == TokenKind::For =>
            {
                Some(self.parse_parallel_for())
            }
            TokenKind::For => Some(self.parse_for()),
            TokenKind::While => Some(self.parse_while()),
            TokenKind::Break => {
                let tok = self.advance().clone();
                Some(Stmt::Break { span: tok.span })
            }
            TokenKind::Continue => {
                let tok = self.advance().clone();
                Some(Stmt::Continue { span: tok.span })
            }
            TokenKind::Spawn => Some(self.parse_spawn()),
            TokenKind::Select => Some(self.parse_select()),
            TokenKind::Try => Some(self.parse_try_catch()),
            TokenKind::Throw => Some(self.parse_throw()),
            TokenKind::Fn => {
                // Peek ahead: if the next token after `fn` is an Ident, this is
                // an inner named function declaration. Desugar to:
                //   let name = fn(params) -> ret { body }
                if self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].kind == TokenKind::Ident
                {
                    Some(self.parse_inner_fn())
                } else {
                    // Anonymous lambda in expression position.
                    Some(self.parse_expr_or_assign())
                }
            }
            TokenKind::RBrace => None, // End of block — caller handles
            _ => Some(self.parse_expr_or_assign()),
        }
    }

    fn parse_let(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::Let);
        let start = kw.span;
        let mutable = self.eat(TokenKind::Mut);

        // Check for pattern destructuring (tuple pattern)
        if self.check(TokenKind::LParen) {
            let pattern = self.parse_pattern();
            let ty = if self.eat(TokenKind::Colon) {
                Some(self.parse_type())
            } else {
                None
            };
            let value = if self.eat(TokenKind::Eq) {
                Some(self.parse_expr())
            } else {
                None
            };
            let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
            return Stmt::Let {
                name: String::new(),
                mutable,
                ty,
                value,
                pattern: Some(pattern),
                span: start.merge(end),
            };
        }

        let (name, _) = self.expect_name();
        let ty = if self.eat(TokenKind::Colon) {
            Some(self.parse_type())
        } else {
            None
        };
        let value = if self.eat(TokenKind::Eq) {
            Some(self.parse_expr())
        } else {
            None
        };
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Stmt::Let {
            name,
            mutable,
            ty,
            value,
            pattern: None,
            span: start.merge(end),
        }
    }

    fn parse_return(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::Return);
        let start = kw.span;
        let value = if !self.check(TokenKind::RBrace) && !self.at_end()
            && !self.check(TokenKind::Semicolon)
        {
            Some(self.parse_expr())
        } else {
            None
        };
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Stmt::Return { value, span: start.merge(end) }
    }

    fn parse_if_stmt(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::If);
        let start = kw.span;
        let condition = self.parse_expr_no_struct_lit();
        let then_block = self.parse_block();

        let mut elif_clauses = Vec::new();
        while self.eat(TokenKind::Elif) {
            let cond = self.parse_expr_no_struct_lit();
            let block = self.parse_block();
            elif_clauses.push((cond, block));
        }

        let else_block = if self.eat(TokenKind::Else) {
            Some(self.parse_block())
        } else {
            None
        };

        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Stmt::If {
            condition,
            then_block,
            elif_clauses,
            else_block,
            span: start.merge(end),
        }
    }

    fn parse_for(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::For);
        let start = kw.span;
        let pattern = self.parse_pattern();
        self.expect(TokenKind::In);
        let iterable = self.parse_expr_no_struct_lit();
        let body = self.parse_block();
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Stmt::For { parallel: false, pattern, iterable, body, span: start.merge(end) }
    }

    fn parse_parallel_for(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::Parallel);
        let start = kw.span;
        self.expect(TokenKind::For);
        let pattern = self.parse_pattern();
        self.expect(TokenKind::In);
        let iterable = self.parse_expr_no_struct_lit();
        let body = self.parse_block();
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Stmt::For { parallel: true, pattern, iterable, body, span: start.merge(end) }
    }

    fn parse_while(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::While);
        let start = kw.span;
        let condition = self.parse_expr_no_struct_lit();
        let body = self.parse_block();
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Stmt::While { condition, body, span: start.merge(end) }
    }

    fn parse_spawn(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::Spawn);
        let start = kw.span;
        let expr = self.parse_expr();
        let end = expr.span();
        Stmt::Spawn { expr, span: start.merge(end) }
    }

    fn parse_select(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::Select);
        let start = kw.span;
        self.expect(TokenKind::LBrace);

        let mut branches = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let tok = self.peek().clone();
            let pattern = tok.text.clone();
            self.advance(); // e.g. "recv"
            let channel = self.parse_expr();
            self.expect(TokenKind::FatArrow);
            let body = self.parse_block();
            let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
            branches.push(SelectBranch {
                pattern,
                channel,
                body,
                span: tok.span.merge(end),
            });
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Stmt::Select { branches, span: start.merge(rbrace.span) }
    }

    fn parse_try_catch(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::Try);
        let start = kw.span;
        let try_block = self.parse_block();
        self.expect(TokenKind::Catch);
        let (catch_name, _) = self.expect_name();
        let catch_block = self.parse_block();
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Stmt::TryCatch {
            try_block,
            catch_name,
            catch_block,
            span: start.merge(end),
        }
    }

    fn parse_throw(&mut self) -> Stmt {
        let kw = self.expect(TokenKind::Throw);
        let start = kw.span;
        let expr = self.parse_expr();
        let end = expr.span();
        Stmt::Throw { expr, span: start.merge(end) }
    }

    /// Parse `fn name(params) -> RetType { body }` inside a function body.
    /// Desugars to `let name = fn(params) -> RetType { body }`.
    fn parse_inner_fn(&mut self) -> Stmt {
        let fn_tok = self.expect(TokenKind::Fn);
        let start = fn_tok.span;
        let (name, _) = self.expect_name();

        self.expect(TokenKind::LParen);
        let params = self.parse_param_list();
        self.expect(TokenKind::RParen);

        let ret_ty = if self.eat(TokenKind::Arrow) {
            Some(self.parse_type())
        } else {
            None
        };

        let body_block = self.parse_block();
        let end = body_block.span;

        let body = if body_block.stmts.len() == 1 {
            if let Stmt::Expr { ref expr, .. } = body_block.stmts[0] {
                expr.clone()
            } else {
                Expr::Block { block: body_block.clone(), span: body_block.span }
            }
        } else {
            Expr::Block { block: body_block.clone(), span: body_block.span }
        };

        let lambda = Expr::Lambda {
            params,
            ret_ty,
            body: Box::new(body),
            span: start.merge(end),
        };

        Stmt::Let {
            name,
            mutable: false,
            ty: None,
            value: Some(lambda),
            pattern: None,
            span: start.merge(end),
        }
    }

    fn parse_expr_or_assign(&mut self) -> Stmt {
        let expr = self.parse_expr();
        let span_start = expr.span();

        // Check for assignment operators
        let assign_op = match self.peek_kind() {
            TokenKind::Eq => Some(AssignOp::Assign),
            TokenKind::PlusEq => Some(AssignOp::AddAssign),
            TokenKind::MinusEq => Some(AssignOp::SubAssign),
            TokenKind::StarEq => Some(AssignOp::MulAssign),
            TokenKind::SlashEq => Some(AssignOp::DivAssign),
            _ => None,
        };

        if let Some(op) = assign_op {
            self.advance();
            let value = self.parse_expr();
            let end = value.span();
            Stmt::Assign {
                target: expr,
                op,
                value,
                span: span_start.merge(end),
            }
        } else {
            let end = expr.span();
            Stmt::Expr { expr, span: span_start.merge(end) }
        }
    }

    // -----------------------------------------------------------------------
    // Expressions — Pratt parser
    // -----------------------------------------------------------------------

    pub fn parse_expr(&mut self) -> Expr {
        self.parse_expr_bp(0)
    }

    /// Parse an expression with struct-literal suppression.
    /// Used in positions where `{` after an identifier should be treated as
    /// a block opener (if/while/for/match conditions), not a struct literal.
    fn parse_expr_no_struct_lit(&mut self) -> Expr {
        let prev = self.no_struct_literal;
        self.no_struct_literal = true;
        let expr = self.parse_expr();
        self.no_struct_literal = prev;
        expr
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Expr {
        let mut lhs = self.parse_prefix();

        loop {
            if self.at_end() {
                break;
            }

            let kind = self.peek_kind();

            // Postfix operators: `.field`, `[index]`, `(call)`
            if kind == TokenKind::Dot && POSTFIX_BP >= min_bp {
                self.advance(); // eat `.`
                let (field, field_span) = self.expect_name();
                let start = lhs.span();

                // Check for method call: `obj.method(args)`
                if self.check(TokenKind::LParen) {
                    self.advance();
                    let args = self.parse_arg_list();
                    let rparen = self.expect(TokenKind::RParen);
                    lhs = Expr::MethodCall {
                        object: Box::new(lhs),
                        method: field,
                        args,
                        span: start.merge(rparen.span),
                    };
                } else {
                    lhs = Expr::FieldAccess {
                        object: Box::new(lhs),
                        field,
                        span: start.merge(field_span),
                    };
                }
                continue;
            }

            if kind == TokenKind::LBracket && POSTFIX_BP >= min_bp {
                self.advance();
                let index = self.parse_expr();
                let rbracket = self.expect(TokenKind::RBracket);
                let start = lhs.span();
                lhs = Expr::IndexAccess {
                    object: Box::new(lhs),
                    index: Box::new(index),
                    span: start.merge(rbracket.span),
                };
                continue;
            }

            if kind == TokenKind::LParen && POSTFIX_BP >= min_bp {
                self.advance();
                let args = self.parse_arg_list();
                let rparen = self.expect(TokenKind::RParen);
                let start = lhs.span();
                lhs = Expr::FnCall {
                    callee: Box::new(lhs),
                    args,
                    span: start.merge(rparen.span),
                };
                continue;
            }

            // Range operators (special: very low precedence, and both sides are optional)
            if (kind == TokenKind::DotDot || kind == TokenKind::DotDotEq) && min_bp <= 1 {
                let inclusive = kind == TokenKind::DotDotEq;
                self.advance();
                let start = lhs.span();
                // Try to parse right side
                let end_expr = if !self.at_end()
                    && !self.check(TokenKind::RBrace)
                    && !self.check(TokenKind::RBracket)
                    && !self.check(TokenKind::RParen)
                    && !self.check(TokenKind::Comma)
                    && !self.check(TokenKind::Semicolon)
                    && !self.check(TokenKind::LBrace)
                {
                    Some(Box::new(self.parse_expr_bp(2)))
                } else {
                    None
                };
                let span_end = end_expr.as_ref()
                    .map(|e| e.span())
                    .unwrap_or(self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span);
                lhs = Expr::RangeExpr {
                    start: Some(Box::new(lhs)),
                    end: end_expr,
                    inclusive,
                    span: start.merge(span_end),
                };
                continue;
            }

            // Infix operators
            let next_kind = if kind == TokenKind::Pipe { Some(self.peek_nth(1)) } else { None };
            if let Some((l_bp, r_bp)) = infix_binding_power(kind, next_kind) {
                if l_bp < min_bp {
                    break;
                }

                // Handle pipe `|>` as two tokens
                if kind == TokenKind::Pipe && next_kind == Some(TokenKind::Gt) {
                    self.advance(); // eat `|`
                    self.advance(); // eat `>`
                    let rhs = self.parse_expr_bp(r_bp);
                    let start = lhs.span();
                    let end = rhs.span();
                    lhs = Expr::PipeExpr {
                        left: Box::new(lhs),
                        right: Box::new(rhs),
                        span: start.merge(end),
                    };
                    continue;
                }

                // Handle `as` cast
                if kind == TokenKind::As {
                    self.advance();
                    let ty = self.parse_type();
                    let start = lhs.span();
                    let end = ty.span();
                    lhs = Expr::Cast {
                        expr: Box::new(lhs),
                        ty,
                        span: start.merge(end),
                    };
                    continue;
                }

                self.advance();
                let rhs = self.parse_expr_bp(r_bp);
                let op = token_to_binop(kind);
                let start = lhs.span();
                let end = rhs.span();
                lhs = Expr::BinaryOp {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                    span: start.merge(end),
                };
                continue;
            }

            break;
        }

        lhs
    }

    fn parse_prefix(&mut self) -> Expr {
        let kind = self.peek_kind();

        // Unary prefix operators
        if let Some(bp) = prefix_binding_power(kind) {
            let tok = self.advance().clone();
            let start = tok.span;

            match kind {
                TokenKind::Minus => {
                    let operand = self.parse_expr_bp(bp);
                    let end = operand.span();
                    Expr::UnaryOp { op: UnOp::Neg, operand: Box::new(operand), span: start.merge(end) }
                }
                TokenKind::Not => {
                    let operand = self.parse_expr_bp(bp);
                    let end = operand.span();
                    Expr::UnaryOp { op: UnOp::Not, operand: Box::new(operand), span: start.merge(end) }
                }
                TokenKind::Tilde => {
                    let operand = self.parse_expr_bp(bp);
                    let end = operand.span();
                    Expr::UnaryOp { op: UnOp::BitNot, operand: Box::new(operand), span: start.merge(end) }
                }
                TokenKind::Amp => {
                    // &x → Borrow (immutable reference)
                    // &mut x → Borrow (mutable reference)
                    let mutable = self.check(TokenKind::Mut);
                    if mutable {
                        self.advance(); // consume `mut`
                    }
                    let inner = self.parse_expr_bp(bp);
                    let end = inner.span();
                    Expr::Borrow { inner: Box::new(inner), mutable, span: start.merge(end) }
                }
                TokenKind::Star => {
                    // *x → Deref (dereference a reference/pointer)
                    let inner = self.parse_expr_bp(bp);
                    let end = inner.span();
                    Expr::Deref { inner: Box::new(inner), span: start.merge(end) }
                }
                TokenKind::Shared => {
                    let inner = self.parse_expr_bp(bp);
                    let end = inner.span();
                    Expr::SharedExpr { inner: Box::new(inner), span: start.merge(end) }
                }
                TokenKind::Move => {
                    let inner = self.parse_expr_bp(bp);
                    let end = inner.span();
                    Expr::MoveExpr { inner: Box::new(inner), span: start.merge(end) }
                }
                TokenKind::Weak => {
                    let inner = self.parse_expr_bp(bp);
                    let end = inner.span();
                    Expr::WeakExpr { inner: Box::new(inner), span: start.merge(end) }
                }
                _ => unreachable!(),
            }
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Expr {
        let tok = self.peek().clone();
        match tok.kind {
            // Literals
            TokenKind::Integer => {
                self.advance();
                let value = parse_int_literal(&tok.text);
                Expr::IntLiteral { value, span: tok.span }
            }
            TokenKind::Float => {
                self.advance();
                let value: f64 = tok.text.replace('_', "").parse().unwrap_or(0.0);
                Expr::FloatLiteral { value, span: tok.span }
            }
            TokenKind::String => {
                self.advance();
                Expr::StringLiteral { value: tok.text.clone(), span: tok.span }
            }
            TokenKind::StringPart => {
                // Interpolated string: collect StringPart and InterpStart/End sequences.
                let start_span = tok.span;
                let mut parts: Vec<StringPart> = Vec::new();

                // First string part (text before first interpolation).
                let first_text = tok.text.clone();
                self.advance();
                if !first_text.is_empty() {
                    parts.push(StringPart::Literal(first_text));
                }

                // Consume alternating interp blocks and string parts.
                while self.check(TokenKind::InterpStart) {
                    self.advance(); // consume InterpStart '{'
                    let expr = self.parse_expr();
                    parts.push(StringPart::Expr(Box::new(expr)));
                    if self.check(TokenKind::InterpEnd) {
                        self.advance(); // consume InterpEnd '}'
                    }
                    // Next StringPart (text after interpolation).
                    if self.check(TokenKind::StringPart) {
                        let text = self.peek().text.clone();
                        self.advance();
                        if !text.is_empty() {
                            parts.push(StringPart::Literal(text));
                        }
                    }
                }

                Expr::InterpolatedString {
                    parts,
                    span: start_span,
                }
            }
            TokenKind::Char => {
                self.advance();
                let ch = tok.text.chars().next().unwrap_or('\0');
                Expr::CharLiteral { value: ch, span: tok.span }
            }
            TokenKind::True => {
                self.advance();
                Expr::BoolLiteral { value: true, span: tok.span }
            }
            TokenKind::False => {
                self.advance();
                Expr::BoolLiteral { value: false, span: tok.span }
            }
            TokenKind::None => {
                self.advance();
                Expr::NoneLiteral { span: tok.span }
            }

            // Identifier — may lead to struct literal, path expression, etc.
            TokenKind::Ident | TokenKind::TypeIdent => {
                self.advance();
                let name = tok.text.clone();
                let start = tok.span;

                // Check for struct literal: `Name { field: value, ... }`
                // But only if this looks like TypeIdent or capitalized name
                if self.check(TokenKind::LBrace) && looks_like_type_name(&name) && !self.no_struct_literal {
                    return self.parse_struct_literal(name, start);
                }

                // Check for static method call: `Type::method(args)`
                if self.check(TokenKind::ColonColon) {
                    self.advance(); // consume ::
                    let (method, _) = self.expect_name();
                    if self.check(TokenKind::LParen) {
                        self.advance(); // consume (
                        let args = self.parse_arg_list();
                        let end = self.expect(TokenKind::RParen);
                        return Expr::StaticMethodCall {
                            type_name: name,
                            method,
                            args,
                            span: start.merge(end.span),
                        };
                    }
                    // Not a call — treat as enum variant: Name::Variant
                    return Expr::Identifier {
                        name: format!("{name}::{method}"),
                        span: start,
                    };
                }

                Expr::Identifier { name, span: start }
            }

            // Channel/send/recv keywords used as function calls in expressions.
            TokenKind::Chan | TokenKind::Send | TokenKind::Recv => {
                self.advance();
                Expr::Identifier { name: tok.text.clone(), span: tok.span }
            }

            // Lambda: `fn(params) -> RetType { body }` or `fn(params) { body }`
            TokenKind::Fn => self.parse_lambda(),

            // If expression
            TokenKind::If => self.parse_if_expr(),

            // Match expression
            TokenKind::Match => self.parse_match_expr(),

            // Comptime block: `comptime { expr }`
            TokenKind::Comptime => {
                self.advance();
                let body = self.parse_block();
                let end = body.span;
                Expr::ComptimeBlock { body, span: tok.span.merge(end) }
            }

            // Quantum block: `quantum { expr }`
            TokenKind::Quantum => {
                self.advance();
                let body = self.parse_block();
                let end = body.span;
                Expr::QuantumBlock { body, span: tok.span.merge(end) }
            }

            // Array literal: `[1, 2, 3]`
            TokenKind::LBracket => self.parse_array_literal(),

            // Grouped expression or tuple: `(expr)` or `(a, b)`
            TokenKind::LParen => self.parse_paren_or_tuple(),

            // Map literal: `{ key: value }`
            TokenKind::LBrace => self.parse_map_or_block_expr(),

            _ => {
                let span = tok.span;
                self.error(format!("expected expression, found {:?}", tok.kind), span);
                self.advance();
                Expr::Identifier { name: "<error>".to_string(), span }
            }
        }
    }

    fn parse_struct_literal(&mut self, name: String, start: Span) -> Expr {
        self.expect(TokenKind::LBrace);
        let mut fields = Vec::new();

        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let (fname, _) = self.expect_name();
            self.expect(TokenKind::Colon);
            let value = self.parse_expr();
            fields.push((fname, value));
            if !self.check(TokenKind::RBrace) {
                self.expect(TokenKind::Comma);
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Expr::StructLiteral {
            name,
            fields,
            span: start.merge(rbrace.span),
        }
    }

    fn parse_lambda(&mut self) -> Expr {
        let fn_tok = self.expect(TokenKind::Fn);
        let start = fn_tok.span;
        self.expect(TokenKind::LParen);
        let params = self.parse_param_list();
        self.expect(TokenKind::RParen);

        let ret_ty = if self.eat(TokenKind::Arrow) {
            Some(self.parse_type())
        } else {
            None
        };

        let body_block = self.parse_block();
        let end = body_block.span;
        // If the block has exactly one expression statement, use that as the body expr.
        // Otherwise, wrap in Expr::Block.
        let body = if body_block.stmts.len() == 1 {
            if let Stmt::Expr { ref expr, .. } = body_block.stmts[0] {
                expr.clone()
            } else {
                Expr::Block { block: body_block.clone(), span: body_block.span }
            }
        } else {
            Expr::Block { block: body_block.clone(), span: body_block.span }
        };

        Expr::Lambda {
            params,
            ret_ty,
            body: Box::new(body),
            span: start.merge(end),
        }
    }

    fn parse_if_expr(&mut self) -> Expr {
        let kw = self.expect(TokenKind::If);
        let start = kw.span;
        let condition = self.parse_expr_no_struct_lit();
        let then_branch = self.parse_block();
        let else_branch = if self.eat(TokenKind::Else) {
            Some(self.parse_block())
        } else {
            None
        };
        let end = self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span;
        Expr::IfExpr {
            condition: Box::new(condition),
            then_branch,
            else_branch,
            span: start.merge(end),
        }
    }

    fn parse_match_expr(&mut self) -> Expr {
        let kw = self.expect(TokenKind::Match);
        let start = kw.span;
        let subject = self.parse_expr_no_struct_lit();
        self.expect(TokenKind::LBrace);

        let mut arms = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let pattern = self.parse_pattern();
            let guard = if self.eat(TokenKind::If) {
                Some(Box::new(self.parse_expr()))
            } else {
                None
            };
            self.expect(TokenKind::FatArrow);
            let body = Box::new(self.parse_expr());
            let arm_end = body.span();
            arms.push(MatchArm {
                pattern,
                guard,
                body,
                span: start.merge(arm_end),
            });
            if !self.check(TokenKind::RBrace) {
                self.eat(TokenKind::Comma);
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Expr::MatchExpr {
            subject: Box::new(subject),
            arms,
            span: start.merge(rbrace.span),
        }
    }

    fn parse_array_literal(&mut self) -> Expr {
        let lbracket = self.expect(TokenKind::LBracket);
        let start = lbracket.span;
        let mut elements = Vec::new();

        while !self.check(TokenKind::RBracket) && !self.at_end() {
            elements.push(self.parse_expr());
            if !self.check(TokenKind::RBracket) {
                self.expect(TokenKind::Comma);
            }
        }
        let rbracket = self.expect(TokenKind::RBracket);
        Expr::ArrayLiteral { elements, span: start.merge(rbracket.span) }
    }

    fn parse_paren_or_tuple(&mut self) -> Expr {
        let lparen = self.expect(TokenKind::LParen);
        let start = lparen.span;

        if self.check(TokenKind::RParen) {
            let rparen = self.expect(TokenKind::RParen);
            return Expr::TupleLiteral { elements: Vec::new(), span: start.merge(rparen.span) };
        }

        let first = self.parse_expr();

        if self.check(TokenKind::Comma) {
            // Tuple literal
            let mut elements = vec![first];
            while self.eat(TokenKind::Comma) {
                if self.check(TokenKind::RParen) {
                    break;
                }
                elements.push(self.parse_expr());
            }
            let rparen = self.expect(TokenKind::RParen);
            Expr::TupleLiteral { elements, span: start.merge(rparen.span) }
        } else {
            // Grouped expression
            self.expect(TokenKind::RParen);
            first
        }
    }

    fn parse_map_or_block_expr(&mut self) -> Expr {
        let lbrace = self.expect(TokenKind::LBrace);
        let start = lbrace.span;

        // Empty map/block
        if self.check(TokenKind::RBrace) {
            let rbrace = self.expect(TokenKind::RBrace);
            return Expr::MapLiteral { entries: Vec::new(), span: start.merge(rbrace.span) };
        }

        // Peek ahead to distinguish map `{key: value}` from block `{ stmts }`
        // If we see `ident :` or `literal :`, it's a map.
        if (self.check(TokenKind::Ident) || self.check(TokenKind::TypeIdent)
            || self.check(TokenKind::String) || self.check(TokenKind::Integer))
            && self.peek_nth(1) == TokenKind::Colon
        {
            return self.parse_map_literal_body(start);
        }

        // Otherwise parse as block expression
        let mut stmts = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            match self.parse_statement() {
                Some(stmt) => stmts.push(stmt),
                None => {
                    if !self.check(TokenKind::RBrace) && !self.at_end() {
                        self.advance();
                    }
                }
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        let block = Block { stmts, span: start.merge(rbrace.span) };
        Expr::Block { block: block.clone(), span: block.span }
    }

    fn parse_map_literal_body(&mut self, start: Span) -> Expr {
        let mut entries = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.at_end() {
            let key = self.parse_expr();
            self.expect(TokenKind::Colon);
            let value = self.parse_expr();
            entries.push((key, value));
            if !self.check(TokenKind::RBrace) {
                self.eat(TokenKind::Comma);
            }
        }
        let rbrace = self.expect(TokenKind::RBrace);
        Expr::MapLiteral { entries, span: start.merge(rbrace.span) }
    }

    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        while !self.check(TokenKind::RParen) && !self.at_end() {
            args.push(self.parse_expr());
            if !self.check(TokenKind::RParen) {
                self.expect(TokenKind::Comma);
            }
        }
        args
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    fn parse_pattern(&mut self) -> Pattern {
        let tok = self.peek().clone();
        match tok.kind {
            // Wildcard: `_`
            TokenKind::Ident if tok.text == "_" => {
                self.advance();
                Pattern::Wildcard { span: tok.span }
            }
            // `mut name`
            TokenKind::Mut => {
                self.advance();
                let (name, span) = self.expect_name();
                Pattern::Ident { name, mutable: true, span: tok.span.merge(span) }
            }
            // Tuple pattern: `(a, b, c)`
            TokenKind::LParen => {
                self.advance();
                let mut elements = Vec::new();
                while !self.check(TokenKind::RParen) && !self.at_end() {
                    elements.push(self.parse_pattern());
                    if !self.check(TokenKind::RParen) {
                        self.expect(TokenKind::Comma);
                    }
                }
                let rparen = self.expect(TokenKind::RParen);
                Pattern::Tuple { elements, span: tok.span.merge(rparen.span) }
            }
            // Literal patterns: integers, strings, bools
            TokenKind::Integer => {
                self.advance();
                let value = parse_int_literal(&tok.text);
                Pattern::Literal {
                    expr: Box::new(Expr::IntLiteral { value, span: tok.span }),
                    span: tok.span,
                }
            }
            TokenKind::String => {
                self.advance();
                Pattern::Literal {
                    expr: Box::new(Expr::StringLiteral { value: tok.text.clone(), span: tok.span }),
                    span: tok.span,
                }
            }
            TokenKind::True => {
                self.advance();
                Pattern::Literal {
                    expr: Box::new(Expr::BoolLiteral { value: true, span: tok.span }),
                    span: tok.span,
                }
            }
            TokenKind::False => {
                self.advance();
                Pattern::Literal {
                    expr: Box::new(Expr::BoolLiteral { value: false, span: tok.span }),
                    span: tok.span,
                }
            }
            // Identifier, struct pattern, or enum pattern
            TokenKind::Ident | TokenKind::TypeIdent => {
                self.advance();
                let name = tok.text.clone();

                // Enum pattern: `Name::Variant` or `Name::Variant(fields)`
                if self.eat(TokenKind::ColonColon) {
                    let (variant, variant_span) = self.expect_name();
                    let mut fields = Vec::new();
                    let mut end = variant_span;
                    if self.eat(TokenKind::LParen) {
                        while !self.check(TokenKind::RParen) && !self.at_end() {
                            fields.push(self.parse_pattern());
                            if !self.check(TokenKind::RParen) {
                                self.expect(TokenKind::Comma);
                            }
                        }
                        let rparen = self.expect(TokenKind::RParen);
                        end = rparen.span;
                    }
                    return Pattern::Enum {
                        name,
                        variant,
                        fields,
                        span: tok.span.merge(end),
                    };
                }

                // Struct pattern: `Name { field1, field2 }`
                if self.check(TokenKind::LBrace) && looks_like_type_name(&name) {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(TokenKind::RBrace) && !self.at_end() {
                        let (fname, _) = self.expect_name();
                        // Shorthand: `{ x }` means `{ x: x }`
                        let pat = if self.eat(TokenKind::Colon) {
                            self.parse_pattern()
                        } else {
                            Pattern::Ident { name: fname.clone(), mutable: false, span: self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span }
                        };
                        fields.push((fname, pat));
                        if !self.check(TokenKind::RBrace) {
                            self.eat(TokenKind::Comma);
                        }
                    }
                    let rbrace = self.expect(TokenKind::RBrace);
                    return Pattern::Struct {
                        name,
                        fields,
                        span: tok.span.merge(rbrace.span),
                    };
                }

                // Simple identifier pattern
                Pattern::Ident { name, mutable: false, span: tok.span }
            }
            _ => {
                let span = tok.span;
                self.error(format!("expected pattern, found {:?}", tok.kind), span);
                self.advance();
                Pattern::Wildcard { span }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    pub fn parse_type(&mut self) -> TypeExpr {
        let tok = self.peek().clone();
        match tok.kind {
            // Optional: `?T`
            TokenKind::Ident if tok.text == "?" => {
                self.advance();
                let inner = self.parse_type();
                let end = inner.span();
                TypeExpr::Optional { inner: Box::new(inner), span: tok.span.merge(end) }
            }
            // Reference: `&T` or `&mut T`
            TokenKind::Amp => {
                self.advance();
                let mutable = self.eat(TokenKind::Mut);
                let inner = self.parse_type();
                let end = inner.span();
                TypeExpr::Reference { inner: Box::new(inner), mutable, span: tok.span.merge(end) }
            }
            // Pointer: `*T` or `*mut T`
            TokenKind::Star => {
                self.advance();
                let mutable = self.eat(TokenKind::Mut);
                let inner = self.parse_type();
                let end = inner.span();
                TypeExpr::Pointer { inner: Box::new(inner), mutable, span: tok.span.merge(end) }
            }
            // Shared: `shared T`
            TokenKind::Shared => {
                self.advance();
                let inner = self.parse_type();
                let end = inner.span();
                TypeExpr::Shared { inner: Box::new(inner), span: tok.span.merge(end) }
            }
            // Weak: `weak T` (not in TypeExpr, but for symmetry — store as Weak)
            TokenKind::Weak => {
                self.advance();
                let inner = self.parse_type();
                let end = inner.span();
                TypeExpr::Weak { inner: Box::new(inner), span: tok.span.merge(end) }
            }
            // Function type: `fn(i32, i32) -> i32`
            TokenKind::Fn => {
                self.advance();
                self.expect(TokenKind::LParen);
                let mut params = Vec::new();
                while !self.check(TokenKind::RParen) && !self.at_end() {
                    params.push(self.parse_type());
                    if !self.check(TokenKind::RParen) {
                        self.expect(TokenKind::Comma);
                    }
                }
                self.expect(TokenKind::RParen);
                self.expect(TokenKind::Arrow);
                let ret = self.parse_type();
                let end = ret.span();
                TypeExpr::Function { params, ret: Box::new(ret), span: tok.span.merge(end) }
            }
            // Array type: `[T; N]` or `[T]`
            TokenKind::LBracket => {
                self.advance();
                let element = self.parse_type();
                let size = if self.eat(TokenKind::Semicolon) {
                    let size_tok = self.expect(TokenKind::Integer);
                    Some(parse_int_literal(&size_tok.text) as u64)
                } else {
                    None
                };
                let rbracket = self.expect(TokenKind::RBracket);
                TypeExpr::Array { element: Box::new(element), size, span: tok.span.merge(rbracket.span) }
            }
            // Tuple type: `(T, U, V)`
            TokenKind::LParen => {
                self.advance();
                let mut elements = Vec::new();
                while !self.check(TokenKind::RParen) && !self.at_end() {
                    elements.push(self.parse_type());
                    if !self.check(TokenKind::RParen) {
                        self.expect(TokenKind::Comma);
                    }
                }
                let rparen = self.expect(TokenKind::RParen);
                TypeExpr::Tuple { elements, span: tok.span.merge(rparen.span) }
            }
            // Channel type: `chan<T>`
            TokenKind::Chan => {
                self.advance();
                if self.eat(TokenKind::Lt) {
                    let inner = self.parse_type();
                    let gt = self.expect(TokenKind::Gt);
                    TypeExpr::Generic {
                        name: "chan".to_string(),
                        args: vec![inner],
                        span: tok.span.merge(gt.span),
                    }
                } else {
                    // Bare `chan` without type param.
                    TypeExpr::Simple { name: "chan".to_string(), span: tok.span }
                }
            }
            // Dynamic trait object: `dyn TraitName`
            TokenKind::Dyn => {
                self.advance();
                // Trait names may be Ident (user-defined) or TypeIdent (builtin).
                let name_tok = if self.check(TokenKind::TypeIdent) {
                    self.advance().clone()
                } else {
                    self.expect(TokenKind::Ident).clone()
                };
                let end = name_tok.span;
                TypeExpr::DynTrait { trait_name: name_tok.text.clone(), span: tok.span.merge(end) }
            }
            // Simple or generic type: `i32`, `Vec<i32>`, `Map<String, i32>`
            TokenKind::Ident | TokenKind::TypeIdent => {
                self.advance();
                let name = tok.text.clone();
                if self.eat(TokenKind::Lt) {
                    let mut args = Vec::new();
                    while !self.check(TokenKind::Gt) && !self.at_end() {
                        args.push(self.parse_type());
                        if !self.check(TokenKind::Gt) {
                            self.expect(TokenKind::Comma);
                        }
                    }
                    let gt = self.expect(TokenKind::Gt);
                    TypeExpr::Generic { name, args, span: tok.span.merge(gt.span) }
                } else {
                    TypeExpr::Simple { name, span: tok.span }
                }
            }
            _ => {
                let span = tok.span;
                self.error(format!("expected type, found {:?}", tok.kind), span);
                self.advance();
                TypeExpr::Inferred { span }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn token_to_binop(kind: TokenKind) -> BinOp {
    match kind {
        TokenKind::Plus => BinOp::Add,
        TokenKind::Minus => BinOp::Sub,
        TokenKind::Star => BinOp::Mul,
        TokenKind::Slash => BinOp::Div,
        TokenKind::Percent => BinOp::Mod,
        TokenKind::Power => BinOp::Pow,
        TokenKind::EqEq => BinOp::Eq,
        TokenKind::BangEq => BinOp::Neq,
        TokenKind::Lt => BinOp::Lt,
        TokenKind::Gt => BinOp::Gt,
        TokenKind::LtEq => BinOp::LtEq,
        TokenKind::GtEq => BinOp::GtEq,
        TokenKind::And => BinOp::And,
        TokenKind::Or => BinOp::Or,
        TokenKind::Amp => BinOp::BitAnd,
        TokenKind::Pipe => BinOp::BitOr,
        TokenKind::Caret => BinOp::BitXor,
        TokenKind::Shl => BinOp::Shl,
        TokenKind::Shr => BinOp::Shr,
        _ => unreachable!("not a binary operator: {:?}", kind),
    }
}

fn parse_int_literal(text: &str) -> i64 {
    let clean = text.replace('_', "");
    if clean.starts_with("0x") || clean.starts_with("0X") {
        i64::from_str_radix(&clean[2..], 16).unwrap_or(0)
    } else if clean.starts_with("0b") || clean.starts_with("0B") {
        i64::from_str_radix(&clean[2..], 2).unwrap_or(0)
    } else if clean.starts_with("0o") || clean.starts_with("0O") {
        i64::from_str_radix(&clean[2..], 8).unwrap_or(0)
    } else {
        clean.parse().unwrap_or(0)
    }
}

fn looks_like_type_name(name: &str) -> bool {
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}
