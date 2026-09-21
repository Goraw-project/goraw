//! Абстрактное синтаксическое дерево Goraw.
//! Типы здесь синтаксические (как их написал пользователь). Разрешением
//! в семантические типы занимается модуль `types`/`check`.

use crate::diag::Span;

/// Синтаксическое имя типа.
#[derive(Clone, Debug)]
pub enum TypeExpr {
    /// Именованный тип: i32, f64, bool, void или имя структуры.
    Named(String, Span),
    /// `*T` — неизменяемый сырой указатель.
    Ptr(Box<TypeExpr>, Span),
    /// `*mut T` — изменяемый сырой указатель.
    PtrMut(Box<TypeExpr>, Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named(_, s) | TypeExpr::Ptr(_, s) | TypeExpr::PtrMut(_, s) => *s,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Param>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<Param>,
    pub variadic: bool,
    pub ret: Option<TypeExpr>,
    pub body: Option<Block>, // None => extern
    pub is_unsafe: bool,
    pub is_extern: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    /// `let [mut] name [: T] = expr;`  либо Go-style `name := expr;`
    Let {
        name: String,
        mutable: bool,
        ty: Option<TypeExpr>,
        value: Expr,
        span: Span,
    },
    /// Присваивание: lvalue `op=` не поддерживаем, только `=`.
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    Expr(Expr),
    Return(Option<Expr>, Span),
    If {
        cond: Expr,
        then: Block,
        els: Option<Block>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    /// Go-style `for init; cond; post { }`. Любая из частей может отсутствовать.
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        post: Option<Box<Stmt>>,
        body: Block,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Unsafe(Block, Span),
    /// Инлайн-ассемблер (фаза 2).
    Asm(AsmBlock),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::Return(_, span)
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Break(span)
            | Stmt::Continue(span)
            | Stmt::Unsafe(_, span) => *span,
            Stmt::Expr(e) => e.span(),
            Stmt::Asm(a) => a.span,
        }
    }
}

/// Диалект инлайн-ассемблера.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsmDialect {
    Masm,
    Nasm,
}

#[derive(Clone, Debug)]
pub struct AsmBlock {
    pub dialect: AsmDialect,
    pub inputs: Vec<String>,  // имена переменных-входов
    pub outputs: Vec<String>, // имена переменных-выходов
    pub body: String,         // сырой текст ассемблера
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Bool(bool, Span),
    Str(String, Span),
    Ident(String, Span),
    /// Бинарная операция.
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// Доступ к полю: `expr.field`.
    Field {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
    /// Индексация: `expr[index]`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// Литерал структуры: `Name { field: value, ... }`.
    StructLit {
        name: String,
        fields: Vec<(String, Expr, Span)>,
        span: Span,
    },
    /// Приведение типа: `expr as T`.
    Cast {
        expr: Box<Expr>,
        ty: TypeExpr,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Str(_, s)
            | Expr::Ident(_, s)
            | Expr::Binary { span: s, .. }
            | Expr::Unary { span: s, .. }
            | Expr::Call { span: s, .. }
            | Expr::Field { span: s, .. }
            | Expr::Index { span: s, .. }
            | Expr::StructLit { span: s, .. }
            | Expr::Cast { span: s, .. } => *s,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And, // &&
    Or,  // ||
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,   // -x
    Not,   // !x
    Deref, // *x
    Ref,   // &x  (взятие адреса, всегда даёт *T или *mut T в зависимости от lvalue)
    RefMut,// &mut x
}

/// Корень программы.
#[derive(Clone, Debug)]
pub struct Program {
    pub structs: Vec<StructDef>,
    pub fns: Vec<FnDef>,
}
