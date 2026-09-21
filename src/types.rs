//! Система типов Goraw и таблица сигнатур (`TyCtx`).
//! Типы намеренно близки к LLVM, чтобы кодоген был почти дословным.

use crate::ast::{FnDef, StructDef, TypeExpr};
use crate::diag::{Diagnostic, Span};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Void,
    /// Указатель: тип элемента + признак изменяемости (safe-модель).
    Ptr(Box<Ty>, bool),
    /// Функция-указатель: типы параметров + тип результата.
    FnPtr(Vec<Ty>, Box<Ty>),
    /// Срез `[]T` — fat-pointer (указатель на элементы + длина i64).
    Slice(Box<Ty>),
    Struct(String),
    /// «Отравленный» тип — уже сообщённая ошибка; гасит каскады.
    Err,
}

impl Ty {
    pub fn is_int(&self) -> bool {
        matches!(
            self,
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
        )
    }
    pub fn is_signed(&self) -> bool {
        matches!(self, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
    }
    pub fn is_float(&self) -> bool {
        matches!(self, Ty::F32 | Ty::F64)
    }
    pub fn is_numeric(&self) -> bool {
        self.is_int() || self.is_float()
    }
    pub fn is_ptr(&self) -> bool {
        matches!(self, Ty::Ptr(..))
    }
    pub fn int_bits(&self) -> u32 {
        match self {
            Ty::I8 | Ty::U8 => 8,
            Ty::I16 | Ty::U16 => 16,
            Ty::I32 | Ty::U32 => 32,
            Ty::I64 | Ty::U64 => 64,
            Ty::Bool => 1,
            _ => 0,
        }
    }

    /// Имя типа в текстовом LLVM IR.
    pub fn llvm(&self) -> String {
        match self {
            Ty::I8 | Ty::U8 => "i8".into(),
            Ty::I16 | Ty::U16 => "i16".into(),
            Ty::I32 | Ty::U32 => "i32".into(),
            Ty::I64 | Ty::U64 => "i64".into(),
            Ty::F32 => "float".into(),
            Ty::F64 => "double".into(),
            Ty::Bool => "i1".into(),
            Ty::Void => "void".into(),
            // LLVM 15+ использует непрозрачные указатели.
            Ty::Ptr(..) => "ptr".into(),
            Ty::FnPtr(..) => "ptr".into(),
            Ty::Slice(..) => "%slice".into(),
            Ty::Struct(name) => format!("%struct.{name}"),
            Ty::Err => "i64".into(),
        }
    }

    /// Человекочитаемое имя типа для сообщений.
    pub fn name(&self) -> String {
        match self {
            Ty::I8 => "i8".into(),
            Ty::I16 => "i16".into(),
            Ty::I32 => "i32".into(),
            Ty::I64 => "i64".into(),
            Ty::U8 => "u8".into(),
            Ty::U16 => "u16".into(),
            Ty::U32 => "u32".into(),
            Ty::U64 => "u64".into(),
            Ty::F32 => "f32".into(),
            Ty::F64 => "f64".into(),
            Ty::Bool => "bool".into(),
            Ty::Void => "void".into(),
            Ty::Ptr(inner, true) => format!("*mut {}", inner.name()),
            Ty::Ptr(inner, false) => format!("*{}", inner.name()),
            Ty::FnPtr(params, ret) => {
                let ps: Vec<String> = params.iter().map(|t| t.name()).collect();
                if **ret == Ty::Void {
                    format!("fn({})", ps.join(", "))
                } else {
                    format!("fn({}) -> {}", ps.join(", "), ret.name())
                }
            }
            Ty::Slice(inner) => format!("[]{}", inner.name()),
            Ty::Struct(n) => n.clone(),
            Ty::Err => "<ошибка>".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StructInfo {
    pub fields: Vec<(String, Ty)>,
    pub span: Span,
}

impl StructInfo {
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(n, _)| n == name)
    }
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub variadic: bool,
    pub is_unsafe: bool,
    pub is_extern: bool,
    pub span: Span,
}

/// Глобальная таблица типов: структуры и сигнатуры функций.
pub struct TyCtx {
    pub structs: HashMap<String, StructInfo>,
    pub fns: HashMap<String, FnSig>,
    /// Перечисления: имя -> (вариант -> значение).
    pub enums: HashMap<String, HashMap<String, i64>>,
    /// Порядок объявления структур (для стабильного вывода IR).
    pub struct_order: Vec<String>,
}

impl TyCtx {
    pub fn new() -> TyCtx {
        TyCtx {
            structs: HashMap::new(),
            fns: HashMap::new(),
            enums: HashMap::new(),
            struct_order: Vec::new(),
        }
    }

    /// Разрешает синтаксический тип в семантический. Ошибки складывает в out.
    pub fn resolve(&self, te: &TypeExpr, out: &mut Vec<Diagnostic>) -> Ty {
        match te {
            TypeExpr::Ptr(inner, _) => Ty::Ptr(Box::new(self.resolve(inner, out)), false),
            TypeExpr::PtrMut(inner, _) => Ty::Ptr(Box::new(self.resolve(inner, out)), true),
            TypeExpr::Fn(params, ret, _) => {
                let ps = params.iter().map(|t| self.resolve(t, out)).collect();
                let r = match ret {
                    Some(t) => self.resolve(t, out),
                    None => Ty::Void,
                };
                Ty::FnPtr(ps, Box::new(r))
            }
            TypeExpr::Slice(inner, _) => Ty::Slice(Box::new(self.resolve(inner, out))),
            TypeExpr::Named(name, sp) => match name.as_str() {
                "i8" => Ty::I8,
                "i16" => Ty::I16,
                "i32" => Ty::I32,
                "i64" => Ty::I64,
                "u8" => Ty::U8,
                "u16" => Ty::U16,
                "u32" => Ty::U32,
                "u64" => Ty::U64,
                "f32" => Ty::F32,
                "f64" => Ty::F64,
                "bool" => Ty::Bool,
                "void" => Ty::Void,
                // строка = срез байтов []u8 (с поддержкой строковых литералов)
                "str" => Ty::Slice(Box::new(Ty::U8)),
                other => {
                    if self.structs.contains_key(other) {
                        Ty::Struct(other.to_string())
                    } else if self.enums.contains_key(other) {
                        // перечисления представляются i32 (C-style)
                        Ty::I32
                    } else {
                        out.push(
                            Diagnostic::error("E0020", *sp, format!("неизвестный тип `{other}`"))
                                .with_hint(
                                    "встроенные типы: i8..i64, u8..u64, f32, f64, bool, void, *T, *mut T",
                                ),
                        );
                        Ty::Err
                    }
                }
            },
        }
    }
}

/// Первый проход: собрать структуры и сигнатуры функций (с проверкой типов
/// в них), чтобы поддержать forward-ссылки. Возвращает TyCtx; ошибки в out.
pub fn collect(
    structs: &[StructDef],
    enums: &[crate::ast::EnumDef],
    fns: &[FnDef],
    out: &mut Vec<Diagnostic>,
) -> TyCtx {
    let mut ctx = TyCtx::new();

    // Перечисления регистрируем первыми — на них могут ссылаться поля/типы.
    for e in enums {
        if ctx.enums.contains_key(&e.name) {
            out.push(Diagnostic::error("E0024", e.span, format!("перечисление `{}` объявлено повторно", e.name)));
            continue;
        }
        let mut vmap = HashMap::new();
        for (vn, vv) in &e.variants {
            vmap.insert(vn.clone(), *vv);
        }
        ctx.enums.insert(e.name.clone(), vmap);
    }

    // Сначала регистрируем имена структур (пустыми), чтобы поля могли
    // ссылаться на другие структуры независимо от порядка.
    for s in structs {
        if ctx.structs.contains_key(&s.name) {
            out.push(Diagnostic::error(
                "E0021",
                s.span,
                format!("структура `{}` объявлена повторно", s.name),
            ));
            continue;
        }
        ctx.structs.insert(s.name.clone(), StructInfo { fields: Vec::new(), span: s.span });
        ctx.struct_order.push(s.name.clone());
    }
    // Теперь заполняем поля.
    for s in structs {
        let mut fields = Vec::new();
        for f in &s.fields {
            let ty = ctx.resolve(&f.ty, out);
            fields.push((f.name.clone(), ty));
        }
        if let Some(info) = ctx.structs.get_mut(&s.name) {
            info.fields = fields;
        }
    }

    for f in fns {
        if ctx.fns.contains_key(&f.name) {
            out.push(Diagnostic::error(
                "E0022",
                f.span,
                format!("функция `{}` объявлена повторно", f.name),
            ));
            continue;
        }
        let params = f.params.iter().map(|p| ctx.resolve(&p.ty, out)).collect();
        let ret = match &f.ret {
            Some(t) => ctx.resolve(t, out),
            None => Ty::Void,
        };
        ctx.fns.insert(
            f.name.clone(),
            FnSig {
                params,
                ret,
                variadic: f.variadic,
                is_unsafe: f.is_unsafe,
                is_extern: f.is_extern,
                span: f.span,
            },
        );
    }

    ctx
}
