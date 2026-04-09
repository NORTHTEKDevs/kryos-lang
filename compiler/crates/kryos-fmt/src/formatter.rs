//! Core formatter implementation.
//!
//! Walks every AST node and emits formatted Kryos source code. The design goals are:
//!
//! - **Canonical output**: running the formatter twice is idempotent.
//! - **4-space indentation** at every nesting level.
//! - **80-column wrapping** for function parameter lists.
//! - **One blank line** between top-level declarations.

use kryos_ast::decl::*;
use kryos_ast::expr::*;
use kryos_ast::stmt::*;
use kryos_ast::types::TypeExpr;

/// Maximum line width before the formatter wraps function parameters across lines.
const MAX_LINE_WIDTH: usize = 80;

/// Number of spaces per indentation level.
const INDENT_WIDTH: usize = 4;

/// The Kryos code formatter.
pub struct Formatter {
    buf: String,
    indent: usize,
}

impl Default for Formatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            indent: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Indentation helpers
    // -----------------------------------------------------------------------

    fn push_indent(&mut self) {
        self.indent += 1;
    }

    fn pop_indent(&mut self) {
        debug_assert!(self.indent > 0);
        self.indent -= 1;
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent * INDENT_WIDTH {
            self.buf.push(' ');
        }
    }

    fn write(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    fn writeln(&mut self, s: &str) {
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    fn newline(&mut self) {
        self.buf.push('\n');
    }

    // -----------------------------------------------------------------------
    // Public entry point
    // -----------------------------------------------------------------------

    pub fn format_module(mut self, module: &Module) -> String {
        for (i, decl) in module.declarations.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.fmt_decl(decl);
        }
        // Ensure trailing newline
        if !self.buf.is_empty() && !self.buf.ends_with('\n') {
            self.newline();
        }
        self.buf
    }

    // -----------------------------------------------------------------------
    // Declarations
    // -----------------------------------------------------------------------

    fn fmt_doc_comments(&mut self, docs: &[String]) {
        for line in docs {
            self.write_indent();
            if line.is_empty() {
                self.writeln("///");
            } else {
                self.write("/// ");
                self.writeln(line);
            }
        }
    }

    fn fmt_decl(&mut self, decl: &Decl) {
        // Emit doc comments before the declaration
        match decl {
            Decl::Function { doc_comments, .. } |
            Decl::Struct { doc_comments, .. } |
            Decl::Enum { doc_comments, .. } |
            Decl::Trait { doc_comments, .. } |
            Decl::Impl { doc_comments, .. } |
            Decl::Const { doc_comments, .. } => self.fmt_doc_comments(doc_comments),
            _ => {}
        }

        match decl {
            Decl::Function {
                name,
                generics,
                params,
                ret_ty,
                body,
                public,
                annotations,
                ..
            } => self.fmt_function(name, generics, params, ret_ty, body, *public, annotations),

            Decl::Struct {
                name,
                generics,
                fields,
                public,
                annotations,
                ..
            } => self.fmt_struct(name, generics, fields, *public, annotations),

            Decl::Enum {
                name,
                generics,
                variants,
                public,
                annotations,
                ..
            } => self.fmt_enum(name, generics, variants, *public, annotations),

            Decl::Trait {
                name,
                generics,
                methods,
                public,
                ..
            } => self.fmt_trait(name, generics, methods, *public),

            Decl::Impl {
                target,
                trait_name,
                generics,
                methods,
                ..
            } => self.fmt_impl(target, trait_name, generics, methods),

            Decl::Actor {
                name,
                state_fields,
                handlers,
                annotations,
                ..
            } => self.fmt_actor(name, state_fields, handlers, annotations),

            Decl::TypeAlias {
                name,
                generics,
                ty,
                public,
                ..
            } => self.fmt_type_alias(name, generics, ty, *public),

            Decl::Import { path, .. } => self.fmt_import(path),

            Decl::Extern { abi, items, .. } => self.fmt_extern(abi, items),

            Decl::Const {
                name,
                ty,
                value,
                public,
                ..
            } => self.fmt_const(name, ty, value, *public),
        }
    }

    // -- annotations --------------------------------------------------------

    fn fmt_annotations(&mut self, annotations: &[Annotation]) {
        for ann in annotations {
            self.write_indent();
            self.write("@");
            self.write(&ann.name);
            if !ann.args.is_empty() {
                self.write("(");
                for (i, arg) in ann.args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(arg);
                }
                self.write(")");
            }
            self.newline();
        }
    }

    // -- generics -----------------------------------------------------------

    fn fmt_generics(&mut self, generics: &[GenericParam]) {
        if generics.is_empty() {
            return;
        }
        self.write("<");
        for (i, g) in generics.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&g.name);
            if !g.bounds.is_empty() {
                self.write(": ");
                for (j, b) in g.bounds.iter().enumerate() {
                    if j > 0 {
                        self.write(" + ");
                    }
                    self.write(b);
                }
            }
        }
        self.write(">");
    }

    // -- function -----------------------------------------------------------

    fn fmt_function(
        &mut self,
        name: &str,
        generics: &[GenericParam],
        params: &[Param],
        ret_ty: &Option<TypeExpr>,
        body: &Option<Block>,
        public: bool,
        annotations: &[Annotation],
    ) {
        self.fmt_annotations(annotations);
        self.write_indent();

        // Build the one-line signature to measure length
        let sig_oneline = self.build_fn_signature_oneline(name, generics, params, ret_ty, public);
        let current_indent_width = self.indent * INDENT_WIDTH;

        if current_indent_width + sig_oneline.len() + if body.is_some() { 3 } else { 0 } <= MAX_LINE_WIDTH {
            // Single-line params
            self.write(&sig_oneline);
        } else {
            // Multi-line params
            self.fmt_fn_signature_multiline(name, generics, params, ret_ty, public);
        }

        match body {
            Some(block) => {
                self.writeln(" {");
                self.fmt_block_body(block);
                self.write_indent();
                self.writeln("}");
            }
            None => {
                // Trait method signature without body
                self.newline();
            }
        }
    }

    fn build_fn_signature_oneline(
        &self,
        name: &str,
        generics: &[GenericParam],
        params: &[Param],
        ret_ty: &Option<TypeExpr>,
        public: bool,
    ) -> String {
        let mut s = String::new();
        if public {
            s.push_str("pub ");
        }
        s.push_str("fn ");
        s.push_str(name);
        if !generics.is_empty() {
            s.push('<');
            for (i, g) in generics.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&g.name);
                if !g.bounds.is_empty() {
                    s.push_str(": ");
                    for (j, b) in g.bounds.iter().enumerate() {
                        if j > 0 {
                            s.push_str(" + ");
                        }
                        s.push_str(b);
                    }
                }
            }
            s.push('>');
        }
        s.push('(');
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(&self.fmt_param_to_string(p));
        }
        s.push(')');
        if let Some(ty) = ret_ty {
            s.push_str(" -> ");
            s.push_str(&self.fmt_type_to_string(ty));
        }
        s
    }

    fn fmt_fn_signature_multiline(
        &mut self,
        name: &str,
        generics: &[GenericParam],
        params: &[Param],
        ret_ty: &Option<TypeExpr>,
        public: bool,
    ) {
        if public {
            self.write("pub ");
        }
        self.write("fn ");
        self.write(name);
        self.fmt_generics(generics);
        self.writeln("(");
        self.push_indent();
        for (i, p) in params.iter().enumerate() {
            self.write_indent();
            self.write(&self.fmt_param_to_string(p));
            if i + 1 < params.len() {
                self.write(",");
            }
            self.newline();
        }
        self.pop_indent();
        self.write_indent();
        self.write(")");
        if let Some(ty) = ret_ty {
            self.write(" -> ");
            self.write(&self.fmt_type_to_string(ty));
        }
    }

    fn fmt_param_to_string(&self, param: &Param) -> String {
        let mut s = param.name.clone();
        if let Some(ty) = &param.ty {
            s.push_str(": ");
            s.push_str(&self.fmt_type_to_string(ty));
        }
        if let Some(default) = &param.default {
            s.push_str(" = ");
            s.push_str(&self.fmt_expr_to_string(default));
        }
        s
    }

    // -- struct -------------------------------------------------------------

    fn fmt_struct(
        &mut self,
        name: &str,
        generics: &[GenericParam],
        fields: &[StructField],
        public: bool,
        annotations: &[Annotation],
    ) {
        self.fmt_annotations(annotations);
        self.write_indent();
        if public {
            self.write("pub ");
        }
        self.write("struct ");
        self.write(name);
        self.fmt_generics(generics);
        self.writeln(" {");
        self.push_indent();
        for field in fields {
            self.write_indent();
            if field.public {
                self.write("pub ");
            }
            self.write(&field.name);
            self.write(": ");
            self.write(&self.fmt_type_to_string(&field.ty));
            if let Some(default) = &field.default {
                self.write(" = ");
                self.write(&self.fmt_expr_to_string(default));
            }
            self.newline();
        }
        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    // -- enum ---------------------------------------------------------------

    fn fmt_enum(
        &mut self,
        name: &str,
        generics: &[GenericParam],
        variants: &[EnumVariant],
        public: bool,
        annotations: &[Annotation],
    ) {
        self.fmt_annotations(annotations);
        self.write_indent();
        if public {
            self.write("pub ");
        }
        self.write("enum ");
        self.write(name);
        self.fmt_generics(generics);
        self.writeln(" {");
        self.push_indent();
        for variant in variants {
            self.write_indent();
            self.write(&variant.name);
            if !variant.fields.is_empty() {
                self.write("(");
                for (i, ty) in variant.fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&self.fmt_type_to_string(ty));
                }
                self.write(")");
            }
            self.newline();
        }
        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    // -- trait --------------------------------------------------------------

    fn fmt_trait(
        &mut self,
        name: &str,
        generics: &[GenericParam],
        methods: &[Decl],
        public: bool,
    ) {
        self.write_indent();
        if public {
            self.write("pub ");
        }
        self.write("trait ");
        self.write(name);
        self.fmt_generics(generics);
        self.writeln(" {");
        self.push_indent();
        for (i, method) in methods.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.fmt_decl(method);
        }
        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    // -- impl ---------------------------------------------------------------

    fn fmt_impl(
        &mut self,
        target: &str,
        trait_name: &Option<String>,
        generics: &[GenericParam],
        methods: &[Decl],
    ) {
        self.write_indent();
        self.write("impl");
        self.fmt_generics(generics);
        self.write(" ");
        if let Some(tn) = trait_name {
            self.write(tn);
            self.write(" for ");
        }
        self.write(target);
        self.writeln(" {");
        self.push_indent();
        for (i, method) in methods.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.fmt_decl(method);
        }
        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    // -- actor --------------------------------------------------------------

    fn fmt_actor(
        &mut self,
        name: &str,
        state_fields: &[StructField],
        handlers: &[MessageHandler],
        annotations: &[Annotation],
    ) {
        self.fmt_annotations(annotations);
        self.write_indent();
        self.write("actor ");
        self.write(name);
        self.writeln(" {");
        self.push_indent();

        // State fields
        for field in state_fields {
            self.write_indent();
            if field.public {
                self.write("pub ");
            }
            self.write(&field.name);
            self.write(": ");
            self.write(&self.fmt_type_to_string(&field.ty));
            if let Some(default) = &field.default {
                self.write(" = ");
                self.write(&self.fmt_expr_to_string(default));
            }
            self.newline();
        }

        // Blank line between state and handlers, if both are non-empty
        if !state_fields.is_empty() && !handlers.is_empty() {
            self.newline();
        }

        // Handlers
        for (i, handler) in handlers.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.write_indent();
            self.write("fn ");
            self.write(&handler.name);
            self.write("(");
            for (j, p) in handler.params.iter().enumerate() {
                if j > 0 {
                    self.write(", ");
                }
                self.write(&self.fmt_param_to_string(p));
            }
            self.write(")");
            if let Some(ty) = &handler.ret_ty {
                self.write(" -> ");
                self.write(&self.fmt_type_to_string(ty));
            }
            self.writeln(" {");
            self.fmt_block_body(&handler.body);
            self.write_indent();
            self.writeln("}");
        }

        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    // -- type alias ---------------------------------------------------------

    fn fmt_type_alias(
        &mut self,
        name: &str,
        generics: &[GenericParam],
        ty: &TypeExpr,
        public: bool,
    ) {
        self.write_indent();
        if public {
            self.write("pub ");
        }
        self.write("type ");
        self.write(name);
        self.fmt_generics(generics);
        self.write(" = ");
        self.write(&self.fmt_type_to_string(ty));
        self.newline();
    }

    // -- const --------------------------------------------------------------

    fn fmt_const(&mut self, name: &str, ty: &Option<TypeExpr>, value: &Expr, public: bool) {
        self.write_indent();
        if public {
            self.write("pub ");
        }
        self.write("let ");
        self.write(name);
        if let Some(ty_expr) = ty {
            self.write(": ");
            self.write(&self.fmt_type_to_string(ty_expr));
        }
        self.write(" = ");
        self.write(&self.fmt_expr_to_string(value));
        self.newline();
    }

    // -- import -------------------------------------------------------------

    fn fmt_import(&mut self, path: &ImportPath) {
        self.write_indent();
        self.write("use ");
        self.write(&path.segments.join("::"));
        if !path.items.is_empty() {
            self.write("::{");
            self.write(&path.items.join(", "));
            self.write("}");
        }
        if let Some(alias) = &path.alias {
            self.write(" as ");
            self.write(alias);
        }
        self.newline();
    }

    // -- extern -------------------------------------------------------------

    fn fmt_extern(&mut self, abi: &str, items: &[Decl]) {
        self.write_indent();
        self.write("extern \"");
        self.write(abi);
        self.write("\" ");
        self.writeln("{");
        self.push_indent();
        for item in items {
            self.fmt_decl(item);
        }
        self.pop_indent();
        self.write_indent();
        self.writeln("}");
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    fn fmt_block_body(&mut self, block: &Block) {
        self.push_indent();
        for stmt in &block.stmts {
            self.fmt_stmt(stmt);
        }
        self.pop_indent();
    }

    fn fmt_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                mutable,
                ty,
                value,
                pattern,
                ..
            } => {
                self.write_indent();
                self.write("let ");
                if *mutable {
                    self.write("mut ");
                }
                if let Some(pat) = pattern {
                    self.write(&self.fmt_pattern_to_string(pat));
                } else {
                    self.write(name);
                }
                if let Some(ty) = ty {
                    self.write(": ");
                    self.write(&self.fmt_type_to_string(ty));
                }
                if let Some(val) = value {
                    self.write(" = ");
                    self.write(&self.fmt_expr_to_string(val));
                }
                self.newline();
            }

            Stmt::Assign { target, op, value, .. } => {
                self.write_indent();
                self.write(&self.fmt_expr_to_string(target));
                self.write(" ");
                self.write(match op {
                    AssignOp::Assign => "=",
                    AssignOp::AddAssign => "+=",
                    AssignOp::SubAssign => "-=",
                    AssignOp::MulAssign => "*=",
                    AssignOp::DivAssign => "/=",
                });
                self.write(" ");
                self.write(&self.fmt_expr_to_string(value));
                self.newline();
            }

            Stmt::Return { value, .. } => {
                self.write_indent();
                self.write("return");
                if let Some(val) = value {
                    self.write(" ");
                    self.write(&self.fmt_expr_to_string(val));
                }
                self.newline();
            }

            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                self.write_indent();
                self.write("if ");
                self.write(&self.fmt_expr_to_string(condition));
                self.writeln(" {");
                self.fmt_block_body(then_block);
                self.write_indent();
                self.write("}");

                for (cond, block) in elif_clauses {
                    self.write(" elif ");
                    self.write(&self.fmt_expr_to_string(cond));
                    self.writeln(" {");
                    self.fmt_block_body(block);
                    self.write_indent();
                    self.write("}");
                }

                if let Some(else_blk) = else_block {
                    self.writeln(" else {");
                    self.fmt_block_body(else_blk);
                    self.write_indent();
                    self.writeln("}");
                } else {
                    self.newline();
                }
            }

            Stmt::For {
                parallel,
                pattern,
                iterable,
                body,
                ..
            } => {
                self.write_indent();
                if *parallel {
                    self.write("parallel ");
                }
                self.write("for ");
                self.write(&self.fmt_pattern_to_string(pattern));
                self.write(" in ");
                self.write(&self.fmt_expr_to_string(iterable));
                self.writeln(" {");
                self.fmt_block_body(body);
                self.write_indent();
                self.writeln("}");
            }

            Stmt::While { condition, body, .. } => {
                self.write_indent();
                self.write("while ");
                self.write(&self.fmt_expr_to_string(condition));
                self.writeln(" {");
                self.fmt_block_body(body);
                self.write_indent();
                self.writeln("}");
            }

            Stmt::Break { .. } => {
                self.write_indent();
                self.writeln("break");
            }

            Stmt::Continue { .. } => {
                self.write_indent();
                self.writeln("continue");
            }

            Stmt::Expr { expr, .. } => {
                self.write_indent();
                self.write(&self.fmt_expr_to_string(expr));
                self.newline();
            }

            Stmt::Spawn { expr, .. } => {
                self.write_indent();
                self.write("spawn ");
                self.write(&self.fmt_expr_to_string(expr));
                self.newline();
            }

            Stmt::Select { branches, .. } => {
                self.write_indent();
                self.writeln("select {");
                self.push_indent();
                for branch in branches {
                    self.write_indent();
                    self.write(&branch.pattern);
                    self.write(" <- ");
                    self.write(&self.fmt_expr_to_string(&branch.channel));
                    self.writeln(" {");
                    self.fmt_block_body(&branch.body);
                    self.write_indent();
                    self.writeln("}");
                }
                self.pop_indent();
                self.write_indent();
                self.writeln("}");
            }

            Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
                ..
            } => {
                self.write_indent();
                self.writeln("try {");
                self.fmt_block_body(try_block);
                self.write_indent();
                self.write("} catch ");
                self.write(catch_name);
                self.writeln(" {");
                self.fmt_block_body(catch_block);
                self.write_indent();
                self.writeln("}");
            }

            Stmt::Throw { expr, .. } => {
                self.write_indent();
                self.write("throw ");
                self.write(&self.fmt_expr_to_string(expr));
                self.newline();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expressions (rendered to a String for inline embedding)
    // -----------------------------------------------------------------------

    fn fmt_expr_to_string(&self, expr: &Expr) -> String {
        match expr {
            Expr::IntLiteral { value, .. } => value.to_string(),
            Expr::FloatLiteral { value, .. } => format_float(*value),
            Expr::StringLiteral { value, .. } => format!("\"{}\"", escape_string(value)),
            Expr::InterpolatedString { parts, .. } => {
                let mut s = String::from("\"");
                for part in parts {
                    match part {
                        StringPart::Literal(lit) => s.push_str(&escape_string(lit)),
                        StringPart::Expr(e) => {
                            s.push_str("${");
                            s.push_str(&self.fmt_expr_to_string(e));
                            s.push('}');
                        }
                    }
                }
                s.push('"');
                s
            }
            Expr::CharLiteral { value, .. } => format!("'{}'", escape_char(*value)),
            Expr::BoolLiteral { value, .. } => {
                if *value { "true".to_string() } else { "false".to_string() }
            }
            Expr::NoneLiteral { .. } => "none".to_string(),

            Expr::Identifier { name, .. } => name.clone(),

            Expr::FieldAccess { object, field, .. } => {
                format!("{}.{}", self.fmt_expr_to_string(object), field)
            }

            Expr::IndexAccess { object, index, .. } => {
                format!(
                    "{}[{}]",
                    self.fmt_expr_to_string(object),
                    self.fmt_expr_to_string(index)
                )
            }

            Expr::BinaryOp { op, left, right, .. } => {
                let left_str = self.maybe_paren_left(left, *op);
                let right_str = self.maybe_paren_right(right, *op);
                format!("{} {} {}", left_str, binop_str(*op), right_str)
            }

            Expr::UnaryOp { op, operand, .. } => {
                let op_str = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "not ",
                    UnOp::BitNot => "~",
                };
                let inner = self.fmt_expr_to_string(operand);
                // Parenthesize binary ops inside unary to avoid ambiguity
                if matches!(**operand, Expr::BinaryOp { .. }) {
                    format!("{}({})", op_str, inner)
                } else {
                    format!("{}{}", op_str, inner)
                }
            }

            Expr::FnCall { callee, args, .. } => {
                let callee_str = self.fmt_expr_to_string(callee);
                let args_str: Vec<String> = args.iter().map(|a| self.fmt_expr_to_string(a)).collect();
                format!("{}({})", callee_str, args_str.join(", "))
            }

            Expr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                let obj_str = self.fmt_expr_to_string(object);
                let args_str: Vec<String> = args.iter().map(|a| self.fmt_expr_to_string(a)).collect();
                format!("{}.{}({})", obj_str, method, args_str.join(", "))
            }

            Expr::ArrayLiteral { elements, .. } => {
                let elems: Vec<String> = elements.iter().map(|e| self.fmt_expr_to_string(e)).collect();
                format!("[{}]", elems.join(", "))
            }

            Expr::TupleLiteral { elements, .. } => {
                let elems: Vec<String> = elements.iter().map(|e| self.fmt_expr_to_string(e)).collect();
                if elems.len() == 1 {
                    format!("({},)", elems[0])
                } else {
                    format!("({})", elems.join(", "))
                }
            }

            Expr::MapLiteral { entries, .. } => {
                let items: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}: {}",
                            self.fmt_expr_to_string(k),
                            self.fmt_expr_to_string(v)
                        )
                    })
                    .collect();
                format!("{{{}}}", items.join(", "))
            }

            Expr::StructLiteral { name, fields, .. } => {
                let field_strs: Vec<String> = fields
                    .iter()
                    .map(|(fname, fval)| {
                        format!("{}: {}", fname, self.fmt_expr_to_string(fval))
                    })
                    .collect();
                format!("{} {{ {} }}", name, field_strs.join(", "))
            }

            Expr::Lambda {
                params,
                ret_ty,
                body,
                ..
            } => {
                let mut s = String::from("fn(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&self.fmt_param_to_string(p));
                }
                s.push(')');
                if let Some(ty) = ret_ty {
                    s.push_str(" -> ");
                    s.push_str(&self.fmt_type_to_string(ty));
                }
                s.push_str(" { ");
                s.push_str(&self.fmt_expr_to_string(body));
                s.push_str(" }");
                s
            }

            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let mut s = format!("if {} {{ ", self.fmt_expr_to_string(condition));
                s.push_str(&self.fmt_block_inline(then_branch));
                s.push_str(" }");
                if let Some(else_blk) = else_branch {
                    s.push_str(" else { ");
                    s.push_str(&self.fmt_block_inline(else_blk));
                    s.push_str(" }");
                }
                s
            }

            Expr::MatchExpr { subject, arms, .. } => {
                let mut s = format!("match {} {{\n", self.fmt_expr_to_string(subject));
                for arm in arms {
                    let indent_str = " ".repeat((self.indent + 1) * INDENT_WIDTH);
                    s.push_str(&indent_str);
                    s.push_str(&self.fmt_pattern_to_string(&arm.pattern));
                    if let Some(guard) = &arm.guard {
                        s.push_str(" if ");
                        s.push_str(&self.fmt_expr_to_string(guard));
                    }
                    s.push_str(" => ");
                    s.push_str(&self.fmt_expr_to_string(&arm.body));
                    s.push('\n');
                }
                let indent_str = " ".repeat(self.indent * INDENT_WIDTH);
                s.push_str(&indent_str);
                s.push('}');
                s
            }

            Expr::RangeExpr {
                start,
                end,
                inclusive,
                ..
            } => {
                let mut s = String::new();
                if let Some(start) = start {
                    s.push_str(&self.fmt_expr_to_string(start));
                }
                if *inclusive {
                    s.push_str("..=");
                } else {
                    s.push_str("..");
                }
                if let Some(end) = end {
                    s.push_str(&self.fmt_expr_to_string(end));
                }
                s
            }

            Expr::PipeExpr { left, right, .. } => {
                format!(
                    "{} |> {}",
                    self.fmt_expr_to_string(left),
                    self.fmt_expr_to_string(right)
                )
            }

            Expr::Borrow { inner, mutable: true, .. } => {
                format!("&mut {}", self.fmt_expr_to_string(inner))
            }
            Expr::Borrow { inner, mutable: false, .. } => {
                format!("&{}", self.fmt_expr_to_string(inner))
            }
            Expr::Deref { inner, .. } => {
                format!("*{}", self.fmt_expr_to_string(inner))
            }

            Expr::SharedExpr { inner, .. } => {
                format!("shared {}", self.fmt_expr_to_string(inner))
            }
            Expr::MoveExpr { inner, .. } => {
                format!("move {}", self.fmt_expr_to_string(inner))
            }
            Expr::WeakExpr { inner, .. } => {
                format!("weak {}", self.fmt_expr_to_string(inner))
            }

            Expr::ComptimeBlock { body, .. } => {
                let mut s = String::from("comptime {\n");
                s.push_str(&self.fmt_block_as_string(body));
                let indent_str = " ".repeat(self.indent * INDENT_WIDTH);
                s.push_str(&indent_str);
                s.push('}');
                s
            }

            Expr::QuantumBlock { body, .. } => {
                let mut s = String::from("quantum {\n");
                s.push_str(&self.fmt_block_as_string(body));
                let indent_str = " ".repeat(self.indent * INDENT_WIDTH);
                s.push_str(&indent_str);
                s.push('}');
                s
            }

            Expr::Cast { expr, ty, .. } => {
                format!(
                    "{} as {}",
                    self.fmt_expr_to_string(expr),
                    self.fmt_type_to_string(ty)
                )
            }

            Expr::Block { block, .. } => {
                let mut s = String::from("{\n");
                s.push_str(&self.fmt_block_as_string(block));
                let indent_str = " ".repeat(self.indent * INDENT_WIDTH);
                s.push_str(&indent_str);
                s.push('}');
                s
            }
        }
    }

    /// Format a block's statements as a string (for inline use in expressions).
    fn fmt_block_inline(&self, block: &Block) -> String {
        let stmts: Vec<String> = block
            .stmts
            .iter()
            .map(|s| self.fmt_stmt_to_string(s))
            .collect();
        stmts.join("; ")
    }

    /// Format a block with proper indentation as a string.
    fn fmt_block_as_string(&self, block: &Block) -> String {
        let mut s = String::new();
        let indent_str = " ".repeat((self.indent + 1) * INDENT_WIDTH);
        for stmt in &block.stmts {
            s.push_str(&indent_str);
            s.push_str(&self.fmt_stmt_to_string(stmt));
            s.push('\n');
        }
        s
    }

    /// Format a single statement to string (no leading indent, no trailing newline).
    fn fmt_stmt_to_string(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Let {
                name,
                mutable,
                ty,
                value,
                pattern,
                ..
            } => {
                let mut s = String::from("let ");
                if *mutable {
                    s.push_str("mut ");
                }
                if let Some(pat) = pattern {
                    s.push_str(&self.fmt_pattern_to_string(pat));
                } else {
                    s.push_str(name);
                }
                if let Some(ty) = ty {
                    s.push_str(": ");
                    s.push_str(&self.fmt_type_to_string(ty));
                }
                if let Some(val) = value {
                    s.push_str(" = ");
                    s.push_str(&self.fmt_expr_to_string(val));
                }
                s
            }
            Stmt::Return { value, .. } => {
                let mut s = String::from("return");
                if let Some(val) = value {
                    s.push(' ');
                    s.push_str(&self.fmt_expr_to_string(val));
                }
                s
            }
            Stmt::Expr { expr, .. } => self.fmt_expr_to_string(expr),
            Stmt::Break { .. } => "break".to_string(),
            Stmt::Continue { .. } => "continue".to_string(),
            _ => {
                // For complex statements in inline positions, render a simplified form.
                // This shouldn't happen often (match/if expressions are Expr variants).
                "/* stmt */".to_string()
            }
        }
    }

    // -----------------------------------------------------------------------
    // Operator precedence for parenthesization
    // -----------------------------------------------------------------------

    fn binop_precedence(op: BinOp) -> u8 {
        match op {
            BinOp::Pipe => 1,
            BinOp::Or => 2,
            BinOp::And => 3,
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => 4,
            BinOp::BitOr => 5,
            BinOp::BitXor => 6,
            BinOp::BitAnd => 7,
            BinOp::Shl | BinOp::Shr => 8,
            BinOp::Add | BinOp::Sub => 9,
            BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::MatMul => 10,
            BinOp::Pow => 11,
        }
    }

    fn maybe_paren_left(&self, expr: &Expr, parent_op: BinOp) -> String {
        let inner = self.fmt_expr_to_string(expr);
        if let Expr::BinaryOp { op, .. } = expr {
            if Self::binop_precedence(*op) < Self::binop_precedence(parent_op) {
                return format!("({})", inner);
            }
        }
        inner
    }

    fn maybe_paren_right(&self, expr: &Expr, parent_op: BinOp) -> String {
        let inner = self.fmt_expr_to_string(expr);
        if let Expr::BinaryOp { op, .. } = expr {
            let child_prec = Self::binop_precedence(*op);
            let parent_prec = Self::binop_precedence(parent_op);
            // Parenthesize if child is lower precedence, OR same precedence
            // for left-associative ops (to disambiguate).
            // Pow is right-associative, so skip same-precedence parens for pow-on-right.
            if child_prec < parent_prec {
                return format!("({})", inner);
            }
            if child_prec == parent_prec && parent_op != BinOp::Pow {
                return format!("({})", inner);
            }
        }
        inner
    }

    // -----------------------------------------------------------------------
    // Type expressions
    // -----------------------------------------------------------------------

    fn fmt_type_to_string(&self, ty: &TypeExpr) -> String {
        match ty {
            TypeExpr::Simple { name, .. } => name.clone(),

            TypeExpr::Generic { name, args, .. } => {
                let args_str: Vec<String> =
                    args.iter().map(|a| self.fmt_type_to_string(a)).collect();
                format!("{}<{}>", name, args_str.join(", "))
            }

            TypeExpr::Array {
                element,
                size,
                ..
            } => {
                let elem = self.fmt_type_to_string(element);
                if let Some(sz) = size {
                    format!("[{}; {}]", elem, sz)
                } else {
                    format!("[{}]", elem)
                }
            }

            TypeExpr::Tuple { elements, .. } => {
                let elems: Vec<String> =
                    elements.iter().map(|e| self.fmt_type_to_string(e)).collect();
                format!("({})", elems.join(", "))
            }

            TypeExpr::Function { params, ret, .. } => {
                let params_str: Vec<String> =
                    params.iter().map(|p| self.fmt_type_to_string(p)).collect();
                format!("fn({}) -> {}", params_str.join(", "), self.fmt_type_to_string(ret))
            }

            TypeExpr::Optional { inner, .. } => {
                format!("{}?", self.fmt_type_to_string(inner))
            }

            TypeExpr::Reference { inner, mutable, .. } => {
                if *mutable {
                    format!("&mut {}", self.fmt_type_to_string(inner))
                } else {
                    format!("&{}", self.fmt_type_to_string(inner))
                }
            }

            TypeExpr::Shared { inner, .. } => {
                format!("shared {}", self.fmt_type_to_string(inner))
            }

            TypeExpr::Weak { inner, .. } => {
                format!("weak {}", self.fmt_type_to_string(inner))
            }

            TypeExpr::Pointer { inner, mutable, .. } => {
                if *mutable {
                    format!("*mut {}", self.fmt_type_to_string(inner))
                } else {
                    format!("*{}", self.fmt_type_to_string(inner))
                }
            }

            TypeExpr::DynTrait { trait_name, .. } => format!("dyn {trait_name}"),

            TypeExpr::Inferred { .. } => "_".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    fn fmt_pattern_to_string(&self, pattern: &Pattern) -> String {
        match pattern {
            Pattern::Wildcard { .. } => "_".to_string(),

            Pattern::Ident { name, mutable, .. } => {
                if *mutable {
                    format!("mut {}", name)
                } else {
                    name.clone()
                }
            }

            Pattern::Literal { expr, .. } => self.fmt_expr_to_string(expr),

            Pattern::Tuple { elements, .. } => {
                let elems: Vec<String> = elements
                    .iter()
                    .map(|e| self.fmt_pattern_to_string(e))
                    .collect();
                format!("({})", elems.join(", "))
            }

            Pattern::Struct { name, fields, .. } => {
                let fields_str: Vec<String> = fields
                    .iter()
                    .map(|(fname, fpat)| {
                        let pat_str = self.fmt_pattern_to_string(fpat);
                        if fname == &pat_str {
                            fname.clone()
                        } else {
                            format!("{}: {}", fname, pat_str)
                        }
                    })
                    .collect();
                format!("{} {{ {} }}", name, fields_str.join(", "))
            }

            Pattern::Enum {
                name,
                variant,
                fields,
                ..
            } => {
                if fields.is_empty() {
                    format!("{}::{}", name, variant)
                } else {
                    let fields_str: Vec<String> = fields
                        .iter()
                        .map(|f| self.fmt_pattern_to_string(f))
                        .collect();
                    format!("{}::{}({})", name, variant, fields_str.join(", "))
                }
            }

            Pattern::Or { patterns, .. } => {
                let parts: Vec<String> = patterns
                    .iter()
                    .map(|p| self.fmt_pattern_to_string(p))
                    .collect();
                parts.join(" | ")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Eq => "==",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Pipe => "|>",
        BinOp::MatMul => "@",
    }
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_char(ch: char) -> String {
    match ch {
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        '\n' => "\\n".to_string(),
        '\r' => "\\r".to_string(),
        '\t' => "\\t".to_string(),
        '\0' => "\\0".to_string(),
        _ => ch.to_string(),
    }
}

fn format_float(v: f64) -> String {
    let s = v.to_string();
    // Ensure there is always a decimal point
    if s.contains('.') {
        s
    } else {
        format!("{}.0", s)
    }
}
