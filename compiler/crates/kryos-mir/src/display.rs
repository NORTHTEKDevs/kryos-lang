//! Pretty-printing for MIR — useful for debugging and `--emit mir`.

use std::fmt;

use crate::ir::*;

impl fmt::Display for MirModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, func) in self.functions.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{func}")?;
        }
        Ok(())
    }
}

impl fmt::Display for MirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Signature.
        write!(f, "fn {}(", self.name)?;
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", p.local, p.ty)?;
        }
        writeln!(f, ") -> {} {{", self.ret_ty)?;

        // Locals.
        for local in &self.locals {
            let mutability = if local.mutable { "mut " } else { "" };
            let name = local
                .name
                .as_deref()
                .map(|n| format!(" // {n}"))
                .unwrap_or_default();
            writeln!(f, "    let {mutability}{}: {};{name}", local.id, local.ty)?;
        }
        if !self.locals.is_empty() {
            writeln!(f)?;
        }

        // Blocks.
        for block in &self.blocks {
            writeln!(f, "    {}:", block.id)?;
            for inst in &block.instructions {
                writeln!(f, "        {inst}")?;
            }
            writeln!(f, "        {}", block.terminator)?;
        }

        writeln!(f, "}}")
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instruction::Assign { dest, value } => write!(f, "{dest} = {value}"),
            Instruction::ArcRetain { ptr } => write!(f, "arc_retain({ptr})"),
            Instruction::ArcRelease { ptr } => write!(f, "arc_release({ptr})"),
            Instruction::Drop { local } => write!(f, "drop({local})"),
            Instruction::DropIfNe { old, new, ty, retained_by_store, via_map } => write!(f, "drop_if_ne({old}, {new}: {ty}, retained_by_store={retained_by_store}, via_map={via_map})"),
            Instruction::Spawn { func, args } => {
                write!(f, "spawn {func}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            Instruction::Send { channel, value } => write!(f, "send({channel}, {value})"),
            Instruction::Receive { dest, channel } => write!(f, "{dest} = receive({channel})"),
            Instruction::ActorSpawn {
                dest,
                dispatch_fn,
                state,
            } => {
                write!(f, "{dest} = actor_spawn({dispatch_fn}, {state})")
            }
            Instruction::ActorSend {
                actor,
                handler_tag,
                args,
            } => {
                write!(f, "actor_send({actor}, tag={handler_tag}")?;
                for a in args {
                    write!(f, ", {a}")?;
                }
                write!(f, ")")
            }
            Instruction::ActorStateLoad {
                dest,
                state_ptr,
                field_offset,
            } => {
                write!(
                    f,
                    "{dest} = actor_state_load({state_ptr}, offset={field_offset})"
                )
            }
            Instruction::ActorStateStore {
                state_ptr,
                field_offset,
                value,
            } => {
                write!(
                    f,
                    "actor_state_store({state_ptr}, offset={field_offset}, {value})"
                )
            }
            Instruction::StoreField {
                object,
                field,
                value,
            } => {
                write!(f, "store_field({object}.{field} = {value})")
            }
            Instruction::StoreDeref { ptr, value } => {
                write!(f, "store_deref(*{ptr} = {value})")
            }
            Instruction::Nop => write!(f, "nop"),
            Instruction::DebugLine(line) => write!(f, "dbg_line {line}"),
        }
    }
}

impl fmt::Display for RValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RValue::Use(op) => write!(f, "{op}"),
            RValue::BinOp { op, left, right } => write!(f, "{op}({left}, {right})"),
            RValue::UnOp { op, operand } => write!(f, "{op}({operand})"),
            RValue::Call { func, args } => {
                write!(f, "call {func}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            RValue::CallIndirect { callee, args } => {
                write!(f, "call_indirect {callee}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            RValue::ConstInt(v) => write!(f, "const {v}_i64"),
            RValue::ConstFloat(v) => write!(f, "const {v}_f64"),
            RValue::ConstBool(v) => write!(f, "const {v}"),
            RValue::ConstString(v) => write!(f, "const {:?}", v),
            RValue::ConstNone => write!(f, "const none"),
            RValue::Array(elems) => {
                write!(f, "[")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                write!(f, "]")
            }
            RValue::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                write!(f, ")")
            }
            RValue::Struct { name, fields } => {
                write!(f, "{name} {{ ")?;
                for (i, (n, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{n}: {v}")?;
                }
                write!(f, " }}")
            }
            RValue::Field { object, field } => write!(f, "{object}.{field}"),
            RValue::Index { object, index } => write!(f, "{object}[{index}]"),
            RValue::ArcAlloc { inner } => write!(f, "arc_alloc({inner})"),
            RValue::Cast { operand, ty } => write!(f, "{operand} as {ty}"),
            RValue::EnumVariant {
                enum_name,
                variant_idx,
                fields,
            } => {
                write!(f, "{enum_name}::variant#{variant_idx}(")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{field}")?;
                }
                write!(f, ")")
            }
            RValue::EnumTag { operand } => write!(f, "enum_tag({operand})"),
            RValue::EnumPayload {
                operand,
                enum_name,
                variant_idx,
                field_idx,
            } => {
                write!(
                    f,
                    "enum_payload({operand}, {enum_name}::v{variant_idx}, field={field_idx})"
                )
            }
            RValue::Closure {
                func_name,
                captures,
            } => {
                write!(f, "closure({func_name}, [")?;
                for (i, cap) in captures.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{cap}")?;
                }
                write!(f, "])")
            }
            RValue::Map(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            RValue::StringConcat(parts) => {
                write!(f, "str_concat(")?;
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ")")
            }
            RValue::Range {
                start,
                end,
                inclusive,
            } => {
                let op = if *inclusive { "..=" } else { ".." };
                match (start, end) {
                    (Some(s), Some(e)) => write!(f, "{s}{op}{e}"),
                    (Some(s), None) => write!(f, "{s}{op}"),
                    (None, Some(e)) => write!(f, "{op}{e}"),
                    (None, None) => write!(f, "{op}"),
                }
            }
            RValue::AddrOf { local, mutable } => {
                if *mutable {
                    write!(f, "&mut {local}")
                } else {
                    write!(f, "&{local}")
                }
            }
            RValue::Deref { operand } => write!(f, "*{operand}"),
            RValue::Comptime(inner) => write!(f, "comptime({inner})"),
            RValue::MakeTraitObject {
                value,
                concrete_type,
                trait_name,
            } => {
                write!(
                    f,
                    "make_trait_object({value}, {concrete_type} as dyn {trait_name})"
                )
            }
            RValue::VtableCall {
                object,
                method_index,
                args,
                ..
            } => {
                write!(f, "vtable_call({object}, method#{method_index}")?;
                for a in args {
                    write!(f, ", {a}")?;
                }
                write!(f, ")")
            }
        }
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Local(id) => write!(f, "{id}"),
            Operand::Constant(c) => write!(f, "{c}"),
        }
    }
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Constant::Int(v) => write!(f, "{v}"),
            Constant::Float(v) => write!(f, "{v}"),
            Constant::Bool(v) => write!(f, "{v}"),
            Constant::Str(v) => write!(f, "{:?}", v),
            Constant::None => write!(f, "none"),
        }
    }
}

impl fmt::Display for MirBinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MirBinOp::Add => "add",
            MirBinOp::Sub => "sub",
            MirBinOp::Mul => "mul",
            MirBinOp::Div => "div",
            MirBinOp::Mod => "mod",
            MirBinOp::Pow => "pow",
            MirBinOp::Eq => "eq",
            MirBinOp::Neq => "neq",
            MirBinOp::Lt => "lt",
            MirBinOp::Gt => "gt",
            MirBinOp::LtEq => "lteq",
            MirBinOp::GtEq => "gteq",
            MirBinOp::And => "and",
            MirBinOp::Or => "or",
            MirBinOp::BitAnd => "bitand",
            MirBinOp::BitOr => "bitor",
            MirBinOp::BitXor => "bitxor",
            MirBinOp::Shl => "shl",
            MirBinOp::Shr => "shr",
        };
        write!(f, "{s}")
    }
}

impl fmt::Display for MirUnOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MirUnOp::Neg => "neg",
            MirUnOp::Not => "not",
            MirUnOp::BitNot => "bitnot",
        };
        write!(f, "{s}")
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminator::Return(None) => write!(f, "return"),
            Terminator::Return(Some(op)) => write!(f, "return {op}"),
            Terminator::Goto(target) => write!(f, "goto -> {target}"),
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => write!(f, "branch {cond} -> [{then_block}, {else_block}]"),
            Terminator::Switch {
                value,
                targets,
                default,
            } => {
                write!(f, "switch {value} [")?;
                for (i, (val, bb)) in targets.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{val}: {bb}")?;
                }
                write!(f, ", otherwise: {default}]")
            }
            Terminator::Unreachable => write!(f, "unreachable"),
        }
    }
}
