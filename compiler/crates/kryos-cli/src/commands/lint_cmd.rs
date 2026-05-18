//! `kryos lint` — production-grade source linter.
//!
//! Walks each `.kry` file's AST and reports stylistic + correctness lints.
//! Output is human-readable by default; `--format=json` emits NDJSON one
//! diagnostic per line.
//!
//! Lints emitted:
//!
//! - **L001** large-function — function body exceeds 100 statements
//! - **L003** magic-number — integer literal > 10 that isn't a let-binding RHS
//!   or a common round number (60, 100, 1000, 1024, 86400, ...)
//! - **L005** shadowed-name — `let x` rebinds an outer binding
//! - **L006** todo-comment — `// TODO` / `// FIXME` / `// XXX`

use std::fs;
use std::path::{Path, PathBuf};

use kryos_ast::{Block, Decl, Expr, Module, Stmt};

#[derive(Debug, Clone)]
pub struct LintOptions {
    pub path: Option<String>,
    pub format: String,
    pub enable: Vec<String>,
    pub disable: Vec<String>,
    pub strict: bool,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            path: None,
            format: "pretty".to_string(),
            enable: Vec::new(),
            disable: Vec::new(),
            strict: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LintDiag {
    pub code: &'static str,
    pub file: PathBuf,
    pub line: u32,
    pub message: String,
    pub severity: &'static str,
}

pub fn execute(opts: LintOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("."),
    };
    if !root.exists() {
        return Err(format!("kryos lint: path '{}' does not exist", root.display()));
    }

    let mut files: Vec<PathBuf> = Vec::new();
    if root.is_file() {
        files.push(root.clone());
    } else {
        collect_kry(&root, &mut files);
    }

    let mut diags: Vec<LintDiag> = Vec::new();
    for f in &files {
        lint_file(f, &mut diags);
    }

    diags.retain(|d| {
        if !opts.enable.is_empty() && !opts.enable.iter().any(|e| e == d.code) {
            return false;
        }
        if opts.disable.iter().any(|e| e == d.code) {
            return false;
        }
        true
    });

    match opts.format.as_str() {
        "json" => {
            for d in &diags {
                println!(
                    r#"{{"code":"{}","severity":"{}","file":"{}","line":{},"message":"{}"}}"#,
                    d.code,
                    d.severity,
                    d.file.display().to_string().replace('\\', "/"),
                    d.line,
                    d.message.replace('"', "\\\"")
                );
            }
        }
        _ => {
            for d in &diags {
                let color = match d.severity {
                    "error" => "\x1b[31m",
                    "warn" => "\x1b[33m",
                    _ => "\x1b[36m",
                };
                println!(
                    "{}{}\x1b[0m {} {}:{}: {}",
                    color, d.severity, d.code, d.file.display(), d.line, d.message
                );
            }
            eprintln!();
            eprintln!(
                "kryos lint: {} diagnostic{} across {} file{}",
                diags.len(),
                if diags.len() == 1 { "" } else { "s" },
                files.len(),
                if files.len() == 1 { "" } else { "s" },
            );
        }
    }

    if opts.strict && !diags.is_empty() {
        return Err(format!("{} lint diagnostic(s) in strict mode", diags.len()));
    }
    Ok(())
}

fn collect_kry(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "kry") {
            out.push(p);
        } else if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            collect_kry(&p, out);
        }
    }
}

fn lint_file(path: &Path, out: &mut Vec<LintDiag>) {
    let Ok(source) = fs::read_to_string(path) else { return };

    // L006 — TODO/FIXME/XXX comments via text pass.
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            let body = trimmed.trim_start_matches('/').trim();
            for tag in &["TODO", "FIXME", "XXX"] {
                if body.starts_with(tag) {
                    out.push(LintDiag {
                        code: "L006",
                        file: path.to_path_buf(),
                        line: idx as u32 + 1,
                        message: format!("{tag} comment: {body}"),
                        severity: "info",
                    });
                    break;
                }
            }
        }
    }

    // AST-driven lints.
    let tokens = kryos_lexer::Lexer::new(&source, 0).tokenize();
    let module = match kryos_parser::parse(tokens) {
        Ok(m) => m,
        Err(_) => return,
    };

    lint_large_functions(&source, path, &module, out);
    lint_magic_numbers(&source, path, &module, out);
    lint_shadowed_names(&source, path, &module, out);
}

fn line_of(source: &str, offset: usize) -> u32 {
    let mut line = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            return line;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

fn lint_large_functions(source: &str, path: &Path, module: &Module, out: &mut Vec<LintDiag>) {
    for decl in &module.declarations {
        if let Decl::Function { name, body: Some(b), span, .. } = decl {
            let n = count_stmts_block(b);
            if n > 100 {
                out.push(LintDiag {
                    code: "L001",
                    file: path.to_path_buf(),
                    line: line_of(source, span.start as usize),
                    message: format!("function `{name}` is large ({n} statements). Consider breaking it up."),
                    severity: "warn",
                });
            }
        }
    }
}

fn count_stmts_block(b: &Block) -> usize {
    let mut n = b.stmts.len();
    for s in &b.stmts {
        match s {
            Stmt::If { then_block, elif_clauses, else_block, .. } => {
                n += count_stmts_block(then_block);
                for (_, eb) in elif_clauses {
                    n += count_stmts_block(eb);
                }
                if let Some(e) = else_block {
                    n += count_stmts_block(e);
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                n += count_stmts_block(body);
            }
            _ => {}
        }
    }
    n
}

fn lint_magic_numbers(source: &str, path: &Path, module: &Module, out: &mut Vec<LintDiag>) {
    for decl in &module.declarations {
        if let Decl::Function { body: Some(b), name, .. } = decl {
            walk_block_magic(source, path, name, b, out);
        }
    }
}

fn walk_block_magic(
    source: &str,
    path: &Path,
    fn_name: &str,
    block: &Block,
    out: &mut Vec<LintDiag>,
) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { value: Some(v), .. } => {
                // Let RHS — magic numbers here are fine (they become the named binding).
                let _ = v;
            }
            Stmt::Expr { expr, .. } => walk_expr_magic(source, path, fn_name, expr, out),
            Stmt::Return { value: Some(v), .. } => walk_expr_magic(source, path, fn_name, v, out),
            Stmt::Assign { value, .. } => walk_expr_magic(source, path, fn_name, value, out),
            Stmt::If { condition, then_block, elif_clauses, else_block, .. } => {
                walk_expr_magic(source, path, fn_name, condition, out);
                walk_block_magic(source, path, fn_name, then_block, out);
                for (c, b) in elif_clauses {
                    walk_expr_magic(source, path, fn_name, c, out);
                    walk_block_magic(source, path, fn_name, b, out);
                }
                if let Some(e) = else_block {
                    walk_block_magic(source, path, fn_name, e, out);
                }
            }
            Stmt::While { condition, body, .. } => {
                walk_expr_magic(source, path, fn_name, condition, out);
                walk_block_magic(source, path, fn_name, body, out);
            }
            Stmt::For { iterable, body, .. } => {
                walk_expr_magic(source, path, fn_name, iterable, out);
                walk_block_magic(source, path, fn_name, body, out);
            }
            _ => {}
        }
    }
}

fn walk_expr_magic(
    source: &str,
    path: &Path,
    fn_name: &str,
    expr: &Expr,
    out: &mut Vec<LintDiag>,
) {
    match expr {
        Expr::IntLiteral { value, span } => {
            let v = value.unsigned_abs();
            let common = matches!(v, 100 | 1000 | 1024 | 60 | 24 | 12 | 86_400);
            if value.abs() > 10 && !common {
                out.push(LintDiag {
                    code: "L003",
                    file: path.to_path_buf(),
                    line: line_of(source, span.start as usize),
                    message: format!(
                        "magic number `{value}` in `{fn_name}` — extract into a named constant"
                    ),
                    severity: "info",
                });
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            walk_expr_magic(source, path, fn_name, left, out);
            walk_expr_magic(source, path, fn_name, right, out);
        }
        Expr::UnaryOp { operand, .. } => {
            walk_expr_magic(source, path, fn_name, operand, out);
        }
        Expr::FnCall { args, .. } | Expr::MethodCall { args, .. } | Expr::StaticMethodCall { args, .. } => {
            for a in args {
                walk_expr_magic(source, path, fn_name, a, out);
            }
        }
        Expr::IfExpr { condition, then_branch, else_branch, .. } => {
            walk_expr_magic(source, path, fn_name, condition, out);
            walk_block_magic(source, path, fn_name, then_branch, out);
            if let Some(e) = else_branch {
                walk_block_magic(source, path, fn_name, e, out);
            }
        }
        _ => {}
    }
}

fn lint_shadowed_names(source: &str, path: &Path, module: &Module, out: &mut Vec<LintDiag>) {
    for decl in &module.declarations {
        if let Decl::Function { body: Some(b), .. } = decl {
            let mut scope: Vec<Vec<String>> = vec![Vec::new()];
            walk_block_shadow(source, path, b, &mut scope, out);
        }
    }
}

fn walk_block_shadow(
    source: &str,
    path: &Path,
    block: &Block,
    scope: &mut Vec<Vec<String>>,
    out: &mut Vec<LintDiag>,
) {
    scope.push(Vec::new());
    for stmt in &block.stmts {
        if let Stmt::Let { name, span, .. } = stmt {
            for outer in &scope[..scope.len() - 1] {
                if outer.iter().any(|n| n == name) {
                    out.push(LintDiag {
                        code: "L005",
                        file: path.to_path_buf(),
                        line: line_of(source, span.start as usize),
                        message: format!("`{name}` shadows an outer binding of the same name"),
                        severity: "warn",
                    });
                    break;
                }
            }
            scope.last_mut().unwrap().push(name.clone());
        }
        match stmt {
            Stmt::If { then_block, elif_clauses, else_block, .. } => {
                walk_block_shadow(source, path, then_block, scope, out);
                for (_, b) in elif_clauses {
                    walk_block_shadow(source, path, b, scope, out);
                }
                if let Some(e) = else_block {
                    walk_block_shadow(source, path, e, scope, out);
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                walk_block_shadow(source, path, body, scope, out);
            }
            _ => {}
        }
    }
    scope.pop();
}
