use crate::types::TypeExpr;
use crate::expr::Expr;
use crate::stmt::Stmt;
use crate::decl::{Decl, Module};

pub trait AstVisitor {
    fn visit_module(&mut self, module: &Module) {
        for decl in &module.declarations {
            self.visit_decl(decl);
        }
    }
    fn visit_decl(&mut self, _decl: &Decl) {}
    fn visit_stmt(&mut self, _stmt: &Stmt) {}
    fn visit_expr(&mut self, _expr: &Expr) {}
    fn visit_type(&mut self, _ty: &TypeExpr) {}
}

pub trait AstMutVisitor {
    fn visit_module(&mut self, module: &mut Module) {
        for decl in &mut module.declarations {
            self.visit_decl(decl);
        }
    }
    fn visit_decl(&mut self, _decl: &mut Decl) {}
    fn visit_stmt(&mut self, _stmt: &mut Stmt) {}
    fn visit_expr(&mut self, _expr: &mut Expr) {}
    fn visit_type(&mut self, _ty: &mut TypeExpr) {}
}
