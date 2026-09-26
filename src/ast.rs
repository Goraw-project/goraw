//! Абстрактное синтаксическое дерево Goraw.
//! Типы здесь синтаксические (как их написал пользователь). Разрешением
//! в семантические типы занимается модуль `types`/`check`.

use crate::diag::Span;

/// Синтаксическое имя типа.
#[derive(Clone, Debug)]
pub enum TypeExpr {
    /// Именованный тип: i32, f64, bool, void или имя структуры.
    Named(String, Span),
    /// Дженерик-тип: `Vec<T>`, `Box<i32>`, `Map<K, V>`.
    Generic(String, Vec<TypeExpr>, Span),
    /// `*T` — неизменяемый сырой указатель.
    Ptr(Box<TypeExpr>, Span),
    /// `*mut T` — изменяемый сырой указатель.
    PtrMut(Box<TypeExpr>, Span),
    /// Тип функции-указателя: `fn(T1, T2) -> R`.
    Fn(Vec<TypeExpr>, Option<Box<TypeExpr>>, Span),
    /// Срез `[]T` — fat-pointer (указатель + длина).
    Slice(Box<TypeExpr>, Span),
    /// Массив фиксированного размера `[N]T`.
    Array(Box<TypeExpr>, u64, Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named(_, s)
            | TypeExpr::Generic(_, _, s)
            | TypeExpr::Ptr(_, s)
            | TypeExpr::PtrMut(_, s)
            | TypeExpr::Fn(_, _, s)
            | TypeExpr::Slice(_, s)
            | TypeExpr::Array(_, _, s) => *s,
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
    pub type_params: Vec<String>,
    pub fields: Vec<Param>,
    pub span: Span,
}

/// Глобальная изменяемая переменная: `static NAME: T = const_expr;`.
#[derive(Clone, Debug)]
pub struct StaticDef {
    pub name: String,
    pub ty: TypeExpr,
    pub value: Expr,
    pub span: Span,
}

/// Глобальная константа: `const NAME[: T] = expr;` (свёртка в компайл-тайме).
#[derive(Clone, Debug)]
pub struct ConstDef {
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub value: Expr,
    pub span: Span,
}

/// Перечисление (C-style, представляется i32).
#[derive(Clone, Debug)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<(String, i64)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FnDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub variadic: bool,
    pub ret: Option<TypeExpr>,
    pub body: Option<Block>, // None => extern
    pub is_unsafe: bool,
    pub is_extern: bool,
    /// Функция сгенерирована из test-блока (в теле разрешён `assert`).
    pub is_test: bool,
    pub span: Span,
}

/// Инлайн C/C++ блок верхнего уровня: `c { ... }` или `cpp { ... }`.
#[derive(Clone, Debug)]
pub struct InlineCBlock {
    pub is_cpp: bool,
    pub code: String,
    pub span: Span,
}

/// Обязательный встроенный тест (shadow test): не попадает в релиз, а под
/// `--test` компилируется в отдельную функцию и прогоняется.
#[derive(Clone, Debug)]
pub struct TestDef {
    pub name: String,
    pub body: Block,
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
    /// Итерация по срезу: `for x in slice { }`.
    ForIn {
        var: String,
        iter: Expr,
        body: Block,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Unsafe(Block, Span),
    /// `assert expr;` внутри test-блока: при ложности тест падает.
    Assert(Expr, Span),
    /// `match expr { pat => block, _ => block }` по целым/enum (LLVM switch).
    /// Ветвь-паттерн: `Some(const-выражение)` либо `None` для `_`.
    Match {
        scrut: Expr,
        arms: Vec<(Option<Expr>, Block)>,
        span: Span,
    },
    /// Инлайн-ассемблер (фаза 2).
    Asm(AsmBlock),
    /// Инлайн C/C++ блок внутри функции: `c(inputs: [...], outputs: [...]) { ... }` или `c { ... }`
    InlineC {
        is_cpp: bool,
        inputs: Vec<String>,
        outputs: Vec<String>,
        body: String,
        span: Span,
    },
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
            | Stmt::ForIn { span, .. }
            | Stmt::Break(span)
            | Stmt::Continue(span)
            | Stmt::Assert(_, span)
            | Stmt::Match { span, .. }
            | Stmt::Unsafe(_, span)
            | Stmt::InlineC { span, .. } => *span,
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
    /// Нулевой указатель.
    Null(Span),
    /// Путь к константе перечисления: `Enum::Variant`.
    Path(String, String, Span),
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
    /// Взятие среза: `expr[start..end]`, `expr[start..]`, `expr[..end]`, `expr[..]`.
    Slice {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        span: Span,
    },
    /// Литерал массива: `[e1, e2, ...]`.
    ArrayLit(Vec<Expr>, Span),
    /// if-выражение: `if cond { a } else { b }` (обе ветви дают значение).
    IfExpr {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
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
    /// JIT-блок с рантайм-специализацией:
    /// `jit(captures: [a, b]) { fn execute(...) -> R { ... } }`.
    /// Значение выражения — функция-указатель на скомпилированную в рантайме
    /// специализацию, где захваты `a`, `b` вкомпилированы как константы.
    Jit {
        captures: Vec<(String, Span)>,
        inner: Box<FnDef>,
        span: Span,
    },
    /// Оператор `?`: ранний выход при ошибке / разворачивание значения.
    Try(Box<Expr>, Span),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Str(_, s)
            | Expr::Ident(_, s)
            | Expr::Null(s)
            | Expr::Path(_, _, s)
            | Expr::Binary { span: s, .. }
            | Expr::Unary { span: s, .. }
            | Expr::Call { span: s, .. }
            | Expr::Field { span: s, .. }
            | Expr::Index { span: s, .. }
            | Expr::Slice { span: s, .. }
            | Expr::StructLit { span: s, .. }
            | Expr::Cast { span: s, .. }
            | Expr::ArrayLit(_, s)
            | Expr::IfExpr { span: s, .. }
            | Expr::Jit { span: s, .. }
            | Expr::Try(_, s) => *s,
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
    Neg,    // -x
    Not,    // !x
    BitNot, // ~x
    Deref, // *x
    Ref,   // &x  (взятие адреса, всегда даёт *T или *mut T в зависимости от lvalue)
    RefMut,// &mut x
}

/// Корень программы.
#[derive(Clone, Debug)]
pub struct Program {
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub consts: Vec<ConstDef>,
    pub statics: Vec<StaticDef>,
    pub fns: Vec<FnDef>,
    pub tests: Vec<TestDef>,
    pub c_blocks: Vec<InlineCBlock>,
}
