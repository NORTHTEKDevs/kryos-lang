# Kryos Formal Grammar

EBNF grammar derived from the recursive-descent / Pratt parser in `compiler/crates/kryos-parser/src/parser.rs`.

Conventions: `|` alternation, `( )` grouping, `?` optional, `*` zero-or-more, `+` one-or-more, `"keyword"` terminal.

---

## Module

```ebnf
Module           = Declaration* ;
```

## Declarations

```ebnf
Declaration      = DocComment* Annotation* "pub"? (
                     FunctionDecl
                   | StructDecl
                   | EnumDecl
                   | TraitDecl
                   | ImplDecl
                   | ActorDecl
                   | TypeAlias
                   | ImportDecl
                   | ExternDecl
                   | ConstDecl
                   ) ;

FunctionDecl     = "async"? "fn" IDENT Generics? "(" ParamList? ")" ( "->" Type )? Block? ;

ParamList        = Param ( "," Param )* ","? ;
Param            = SelfParam | NamedParam ;
SelfParam        = "self" ( ":" Type )? ;
NamedParam       = IDENT ":" Type ( "=" Expr )? ;

StructDecl       = "struct" IDENT Generics? "{" StructField* "}" ;
StructField      = "pub"? IDENT ":" Type ( "=" Expr )? ","? ;

EnumDecl         = "enum" IDENT Generics? "{" EnumVariant* "}" ;
EnumVariant      = IDENT ( "(" Type ( "," Type )* ")" )? ","? ;

TraitDecl        = "trait" IDENT Generics? "{" FunctionDecl* "}" ;

ImplDecl         = "impl" Generics? IDENT ( "for" IDENT )? "{" ( "pub"? FunctionDecl )* "}" ;

ActorDecl        = "actor" IDENT "{" ( StateField | FunctionDecl )* "}" ;
StateField       = IDENT ":" Type ","? ;

TypeAlias        = "type" IDENT Generics? "=" Type ;

ConstDecl        = "let" "mut"? IDENT ( ":" Type )? "=" Expr ;

ImportDecl       = "use" ImportPath ;
ImportPath       = IDENT ( "::" IDENT )* ( "::" "{" IDENT ( "," IDENT )* "}" )? ( "as" IDENT )? ;

ExternDecl       = "extern" STRING? "{" FunctionDecl* "}" ;

Annotation       = "@" IDENT ( "(" IDENT ( "," IDENT )* ")" )? ;

DocComment       = "///" .* ;

Generics         = "<" GenericParam ( "," GenericParam )* ">" ;
GenericParam     = IDENT ( ":" Bound ( "+" Bound )* )? ;
Bound            = IDENT ;
```

## Statements

```ebnf
Block            = "{" Stmt* "}" ;

Stmt             = LetStmt
                 | ReturnStmt
                 | IfStmt
                 | ForStmt
                 | WhileStmt
                 | BreakStmt
                 | ContinueStmt
                 | SpawnStmt
                 | SelectStmt
                 | TryCatchStmt
                 | ThrowStmt
                 | InnerFnStmt
                 | ExprOrAssignStmt ;

LetStmt          = "let" "mut"? ( TuplePattern | IDENT ) ( ":" Type )? ( "=" Expr )? ;

ReturnStmt       = "return" Expr? ;

IfStmt           = "if" ExprNoStruct Block
                   ( "else" "if" ExprNoStruct Block )*
                   ( "else" Block )? ;

ForStmt          = "parallel"? "for" Pattern "in" ExprNoStruct Block ;

WhileStmt        = "while" ExprNoStruct Block ;

BreakStmt        = "break" ;

ContinueStmt     = "continue" ;

SpawnStmt        = "spawn" Expr ;

SelectStmt       = "select" "{" SelectBranch* "}" ;
SelectBranch     = IDENT Expr "=>" Block ;

TryCatchStmt     = "try" Block "catch" IDENT Block ;

ThrowStmt        = "throw" Expr ;

InnerFnStmt      = "fn" IDENT "(" ParamList? ")" ( "->" Type )? Block ;
                   (* Desugars to: let IDENT = fn(...) -> ... { body } *)

ExprOrAssignStmt = Expr ( AssignOp Expr )? ;
AssignOp         = "=" | "+=" | "-=" | "*=" | "/=" ;
```

## Expressions

Expressions use Pratt (top-down operator precedence) parsing. `ExprNoStruct` suppresses `Name { ... }` as struct literal so `{` is treated as a block opener in conditions.

```ebnf
Expr             = ExprBp(0) ;
ExprNoStruct     = Expr ;    (* parsed with no_struct_literal = true *)

ExprBp(min_bp)   = PrefixExpr ( InfixOp ExprBp(r_bp) | PostfixOp )* ;

PrefixExpr       = PrefixOp ExprBp(prefix_bp)
                 | PrimaryExpr ;

PrefixOp         = "-" | "!" | "~"
                 | "&" "mut"?
                 | "*"
                 | "shared" | "move" | "weak" | "await" ;

PrimaryExpr      = INT_LITERAL
                 | FLOAT_LITERAL
                 | STRING_LITERAL
                 | InterpolatedString
                 | CHAR_LITERAL
                 | "true" | "false"
                 | "none"
                 | IDENT ( "::" IDENT ( "(" ArgList? ")" )? )?
                 | StructLiteral
                 | Lambda
                 | IfExpr
                 | MatchExpr
                 | "comptime" Block
                 | "quantum" Block
                 | ArrayLiteral
                 | ParenOrTuple
                 | MapLiteral
                 | "chan" | "send" | "recv" ;

StructLiteral    = IDENT "{" ( IDENT ":" Expr ) ( "," IDENT ":" Expr )* ","? "}" ;

Lambda           = "fn" "(" ParamList? ")" ( "->" Type )? Block ;

IfExpr           = "if" ExprNoStruct Block ( "else" ( IfExpr | Block ) )? ;

MatchExpr        = "match" ExprNoStruct "{" MatchArm* "}" ;
MatchArm         = Pattern ( "if" Expr )? "=>" Expr ","? ;

ArrayLiteral     = "[" ( Expr ( "," Expr )* ","? )? "]" ;

ParenOrTuple     = "(" ( Expr ( "," Expr )* )? ")" ;

MapLiteral       = "{" ( Expr ":" Expr ( "," Expr ":" Expr )* ","? )? "}" ;

InterpolatedString = STRING_PART ( "{" Expr "}" STRING_PART )* ;

ArgList          = Expr ( "," Expr )* ","? ;
```

### Infix and Postfix Operators

```ebnf
InfixOp          = BinaryOp | RangeOp | PipeOp | CastOp ;

BinaryOp         = "+" | "-" | "*" | "/" | "%" | "**"
                 | "==" | "!=" | "<" | ">" | "<=" | ">="
                 | "and" | "or"
                 | "|" | "^" | "&"
                 | "<<" | ">>" ;

RangeOp          = ".." | "..=" ;

PipeOp           = "|>" ;

CastOp           = "as" ;

PostfixOp        = "." IDENT ( "(" ArgList? ")" )?    (* field access or method call *)
                 | "[" Expr "]"                        (* index access *)
                 | "(" ArgList? ")"                    (* function call *) ;
```

## Operator Precedence

From lowest to highest binding power:

| Level | Operators           | Associativity | Notes               |
|-------|---------------------|---------------|----------------------|
| 1     | `..` `..=`          | —             | Range                |
| 2     | `\|>`               | Left          | Pipe                 |
| 3     | `or`                | Left          | Logical OR           |
| 4     | `and`               | Left          | Logical AND          |
| 5     | `==` `!=` `<` `>` `<=` `>=` | Left | Comparison           |
| 6     | `\|`                | Left          | Bitwise OR           |
| 7     | `^`                 | Left          | Bitwise XOR          |
| 8     | `&`                 | Left          | Bitwise AND          |
| 9     | `<<` `>>`           | Left          | Bitwise shifts       |
| 10    | `+` `-`             | Left          | Additive             |
| 11    | `*` `/` `%`         | Left          | Multiplicative       |
| 12    | `-` `!` `~` `&` `*` | —            | Unary prefix         |
| 13    | `**`                | Right         | Exponentiation       |
| 14    | `as`                | Left          | Type cast            |
| 15    | `.` `[` `(`         | Left          | Postfix (field, index, call) |

Prefix keyword operators `shared`, `move`, `weak`, `await` bind at level 2 (just above range).

## Patterns

```ebnf
Pattern          = "_"
                 | "mut"? IDENT
                 | INT_LITERAL | STRING_LITERAL | "true" | "false"
                 | IDENT "::" IDENT ( "(" Pattern ( "," Pattern )* ")" )?
                 | IDENT "{" FieldPattern ( "," FieldPattern )* "}"
                 | "(" Pattern ( "," Pattern )* ")" ;

FieldPattern     = IDENT ( ":" Pattern )? ;
```

## Types

```ebnf
Type             = PrimitiveType
                 | "[" Type ( ";" INT_LITERAL )? "]"
                 | "(" Type ( "," Type )* ")"
                 | "fn" "(" ( Type ( "," Type )* )? ")" "->" Type
                 | "chan" ( "<" Type ">" )?
                 | IDENT "<" Type ( "," Type )* ">"
                 | "?" Type
                 | "&" "mut"? Type
                 | "*" "mut"? Type
                 | "shared" Type
                 | "weak" Type
                 | "dyn" IDENT
                 | "Self"
                 | IDENT ;

PrimitiveType    = "i64" | "f64" | "str" | "bool" | "void" ;
```

## Lexical Elements

```ebnf
IDENT            = [a-zA-Z_] [a-zA-Z0-9_]* ;
INT_LITERAL      = [0-9]+ | "0x" [0-9a-fA-F]+ | "0b" [01]+ | "0o" [0-7]+ ;
FLOAT_LITERAL    = [0-9]+ "." [0-9]+ ;
STRING_LITERAL   = '"' ( [^"\\] | '\\' . )* '"' ;
CHAR_LITERAL     = "'" ( [^'\\] | '\\' . ) "'" ;
```

## Reserved Keywords

```
fn async struct enum trait impl actor type let mut return if else
elif for while loop break continue match spawn select try catch
throw use extern pub self Self true false none comptime quantum
shared move weak await and or not as in dyn parallel chan send recv
```
