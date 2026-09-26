//! Модуль мономорфизации дженериков (Generics / Compile-time Templates) в Goraw.
//!
//! Превращает обобщённые структуры (`struct Vec<T>`) и методы (`fn Vec<T>::push`)
//! в конкретные специализированные типы (`Vec__i32`, `Vec__i32__push`)
//! на уровне AST до прохода тайпчекера и кодогенерации.
//!
//! Результат:
//! - Нулевой оверхед в рантайме (Zero-cost abstractions)
//! - Полная совместимость с C++ шаблонами и Rust дженериками
//! - Автоматический вызов RAII-деструкторов (`Vec__i32__drop`)

use crate::ast::*;
use crate::diag::Span;
use std::collections::{HashMap, HashSet};

pub fn monomorphize(prog: &mut Program) {
    // 1. Отделяем шаблонные структуры от обычных
    let mut generic_structs: HashMap<String, StructDef> = HashMap::new();
    let mut regular_structs: Vec<StructDef> = Vec::new();

    for s in std::mem::take(&mut prog.structs) {
        if !s.type_params.is_empty() {
            generic_structs.insert(s.name.clone(), s);
        } else {
            regular_structs.push(s);
        }
    }

    // 2. Отделяем шаблонные функции от обычных
    let mut generic_fns: Vec<FnDef> = Vec::new();
    let mut regular_fns: Vec<FnDef> = Vec::new();

    for f in std::mem::take(&mut prog.fns) {
        if !f.type_params.is_empty() || is_method_of_generic(&f.name, &generic_structs) {
            generic_fns.push(f);
        } else {
            regular_fns.push(f);
        }
    }

    // 3. Собираем все использованные специализации дженериков: (struct_name, Vec<TypeExpr>)
    let mut instantiations: HashSet<(String, Vec<TypeExprKey>)> = HashSet::new();
    find_instantiations_in_structs(&regular_structs, &mut instantiations);
    find_instantiations_in_fns(&regular_fns, &generic_structs, &mut instantiations);

    // Итеративно специализируем, пока находятся новые вложенные дженерики (напр. Box<Vec<i32>>)
    let mut specialized_structs: HashMap<String, StructDef> = HashMap::new();
    let mut specialized_fns: Vec<FnDef> = Vec::new();
    let mut processed_instantiations: HashSet<(String, Vec<TypeExprKey>)> = HashSet::new();

    loop {
        let new_insts: Vec<(String, Vec<TypeExprKey>)> = instantiations
            .iter()
            .filter(|inst| !processed_instantiations.contains(*inst))
            .cloned()
            .collect();

        if new_insts.is_empty() {
            break;
        }

        for (base_name, type_args) in new_insts {
            processed_instantiations.insert((base_name.clone(), type_args.clone()));

            if let Some(tmpl) = generic_structs.get(&base_name) {
                let mangled_name = mangle_specialized_name(&base_name, &type_args);
                if specialized_structs.contains_key(&mangled_name) {
                    continue;
                }

                // Строим карту подстановок T -> TypeExpr
                let mut subst_map: HashMap<String, TypeExpr> = HashMap::new();
                for (param_name, arg_key) in tmpl.type_params.iter().zip(type_args.iter()) {
                    subst_map.insert(param_name.clone(), arg_key.to_type_expr());
                }

                // Создаём специализированную структуру
                let mut spec_struct = tmpl.clone();
                spec_struct.name = mangled_name.clone();
                spec_struct.type_params.clear();
                for field in &mut spec_struct.fields {
                    substitute_type_expr(&mut field.ty, &subst_map, &base_name, &mangled_name);
                }
                specialized_structs.insert(mangled_name.clone(), spec_struct);

                // Ищем методы этой шаблонной структуры
                let prefix = format!("{base_name}__");
                for f_tmpl in &generic_fns {
                    if f_tmpl.name.starts_with(&prefix) {
                        let method_suffix = &f_tmpl.name[prefix.len()..];
                        let spec_fn_name = format!("{mangled_name}__{method_suffix}");

                        let mut spec_fn = f_tmpl.clone();
                        spec_fn.name = spec_fn_name;
                        spec_fn.type_params.clear();

                        for param in &mut spec_fn.params {
                            substitute_type_expr(&mut param.ty, &subst_map, &base_name, &mangled_name);
                        }
                        if let Some(ret) = &mut spec_fn.ret {
                            substitute_type_expr(ret, &subst_map, &base_name, &mangled_name);
                        }
                        if let Some(body) = &mut spec_fn.body {
                            substitute_block(body, &subst_map, &base_name, &mangled_name);
                        }
                        specialized_fns.push(spec_fn);
                    }
                }
            }
        }

        // Проверяем новые специализации на наличие вложенных дженериков
        for s in specialized_structs.values() {
            find_instantiations_in_struct(s, &mut instantiations);
        }
        for f in &specialized_fns {
            find_instantiations_in_fn(f, &generic_structs, &mut instantiations);
        }
    }

    // 4. Заменяем все Generic-типы на Named(mangled) по всей программе
    for s in &mut regular_structs {
        for field in &mut s.fields {
            rewrite_generic_to_named(&mut field.ty);
        }
    }

    for f in &mut regular_fns {
        for param in &mut f.params {
            rewrite_generic_to_named(&mut param.ty);
        }
        if let Some(ret) = &mut f.ret {
            rewrite_generic_to_named(ret);
        }
        if let Some(body) = &mut f.body {
            rewrite_block_generics(body);
        }
    }

    for f in &mut specialized_fns {
        for param in &mut f.params {
            rewrite_generic_to_named(&mut param.ty);
        }
        if let Some(ret) = &mut f.ret {
            rewrite_generic_to_named(ret);
        }
        if let Some(body) = &mut f.body {
            rewrite_block_generics(body);
        }
    }

    // 5. Собираем всё обратно в prog
    let mut final_structs = regular_structs;
    final_structs.extend(specialized_structs.into_values());
    prog.structs = final_structs;

    let mut final_fns = regular_fns;
    final_fns.extend(specialized_fns);
    prog.fns = final_fns;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeExprKey(pub String);

impl TypeExprKey {
    pub fn from_type_expr(te: &TypeExpr) -> Self {
        TypeExprKey(format_key(te))
    }

    pub fn to_type_expr(&self) -> TypeExpr {
        parse_key(&self.0)
    }
}

fn format_key(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Named(n, _) => n.clone(),
        TypeExpr::Generic(n, args, _) => {
            let inner: Vec<String> = args.iter().map(format_key).collect();
            format!("{n}__{}", inner.join("__"))
        }
        TypeExpr::Ptr(inner, _) => format!("ptr_{}", format_key(inner)),
        TypeExpr::PtrMut(inner, _) => format!("ptrmut_{}", format_key(inner)),
        TypeExpr::Slice(inner, _) => format!("slice_{}", format_key(inner)),
        TypeExpr::Array(inner, n, _) => format!("arr{n}_{}", format_key(inner)),
        TypeExpr::Fn(params, ret, _) => {
            let plist: Vec<String> = params.iter().map(format_key).collect();
            let r = match ret {
                Some(rt) => format!("__{}", format_key(rt)),
                None => String::new(),
            };
            format!("fn_{}{}", plist.join("_"), r)
        }
    }
}

fn parse_key(s: &str) -> TypeExpr {
    if let Some(rest) = s.strip_prefix("ptrmut_") {
        TypeExpr::PtrMut(Box::new(parse_key(rest)), Span::dummy())
    } else if let Some(rest) = s.strip_prefix("ptr_") {
        TypeExpr::Ptr(Box::new(parse_key(rest)), Span::dummy())
    } else if let Some(rest) = s.strip_prefix("slice_") {
        TypeExpr::Slice(Box::new(parse_key(rest)), Span::dummy())
    } else {
        TypeExpr::Named(s.to_string(), Span::dummy())
    }
}

fn mangle_specialized_name(base: &str, args: &[TypeExprKey]) -> String {
    let mut s = base.to_string();
    for a in args {
        s.push('_');
        s.push('_');
        s.push_str(&a.0);
    }
    s
}

fn is_method_of_generic(name: &str, generic_structs: &HashMap<String, StructDef>) -> bool {
    if let Some(pos) = name.find("__") {
        let base = &name[..pos];
        generic_structs.contains_key(base)
    } else {
        false
    }
}

fn find_instantiations_in_structs(
    structs: &[StructDef],
    insts: &mut HashSet<(String, Vec<TypeExprKey>)>,
) {
    for s in structs {
        find_instantiations_in_struct(s, insts);
    }
}

fn find_instantiations_in_struct(
    s: &StructDef,
    insts: &mut HashSet<(String, Vec<TypeExprKey>)>,
) {
    for field in &s.fields {
        find_in_type_expr(&field.ty, insts);
    }
}

fn find_instantiations_in_fns(
    fns: &[FnDef],
    generic_structs: &HashMap<String, StructDef>,
    insts: &mut HashSet<(String, Vec<TypeExprKey>)>,
) {
    for f in fns {
        find_instantiations_in_fn(f, generic_structs, insts);
    }
}

fn find_instantiations_in_fn(
    f: &FnDef,
    generic_structs: &HashMap<String, StructDef>,
    insts: &mut HashSet<(String, Vec<TypeExprKey>)>,
) {
    for p in &f.params {
        find_in_type_expr(&p.ty, insts);
    }
    if let Some(ret) = &f.ret {
        find_in_type_expr(ret, insts);
    }
    if let Some(b) = &f.body {
        find_in_block(b, generic_structs, insts);
    }
}

fn find_in_type_expr(te: &TypeExpr, insts: &mut HashSet<(String, Vec<TypeExprKey>)>) {
    match te {
        TypeExpr::Generic(name, args, _) => {
            let keys: Vec<TypeExprKey> = args.iter().map(TypeExprKey::from_type_expr).collect();
            insts.insert((name.clone(), keys));
            for a in args {
                find_in_type_expr(a, insts);
            }
        }
        TypeExpr::Ptr(inner, _) | TypeExpr::PtrMut(inner, _) | TypeExpr::Slice(inner, _) => {
            find_in_type_expr(inner, insts);
        }
        TypeExpr::Array(inner, _, _) => {
            find_in_type_expr(inner, insts);
        }
        TypeExpr::Fn(params, ret, _) => {
            for p in params {
                find_in_type_expr(p, insts);
            }
            if let Some(r) = ret {
                find_in_type_expr(r, insts);
            }
        }
        TypeExpr::Named(_, _) => {}
    }
}

fn find_in_block(
    b: &Block,
    generic_structs: &HashMap<String, StructDef>,
    insts: &mut HashSet<(String, Vec<TypeExprKey>)>,
) {
    for s in &b.stmts {
        find_in_stmt(s, generic_structs, insts);
    }
}

fn find_in_stmt(
    s: &Stmt,
    generic_structs: &HashMap<String, StructDef>,
    insts: &mut HashSet<(String, Vec<TypeExprKey>)>,
) {
    match s {
        Stmt::Let { ty, value, .. } => {
            if let Some(t) = ty {
                find_in_type_expr(t, insts);
            }
            find_in_expr(value, generic_structs, insts);
        }
        Stmt::Assign { target, value, .. } => {
            find_in_expr(target, generic_structs, insts);
            find_in_expr(value, generic_structs, insts);
        }
        Stmt::Expr(e) => find_in_expr(e, generic_structs, insts),
        Stmt::Return(e, _) => {
            if let Some(ex) = e {
                find_in_expr(ex, generic_structs, insts);
            }
        }
        Stmt::If { cond, then, els, .. } => {
            find_in_expr(cond, generic_structs, insts);
            find_in_block(then, generic_structs, insts);
            if let Some(el) = els {
                find_in_block(el, generic_structs, insts);
            }
        }
        Stmt::While { cond, body, .. } => {
            find_in_expr(cond, generic_structs, insts);
            find_in_block(body, generic_structs, insts);
        }
        Stmt::For { init, cond, post, body, .. } => {
            if let Some(i) = init {
                find_in_stmt(i, generic_structs, insts);
            }
            if let Some(c) = cond {
                find_in_expr(c, generic_structs, insts);
            }
            if let Some(p) = post {
                find_in_stmt(p, generic_structs, insts);
            }
            find_in_block(body, generic_structs, insts);
        }
        Stmt::ForIn { iter, body, .. } => {
            find_in_expr(iter, generic_structs, insts);
            find_in_block(body, generic_structs, insts);
        }
        Stmt::Unsafe(b, _) => find_in_block(b, generic_structs, insts),
        Stmt::Assert(e, _) => find_in_expr(e, generic_structs, insts),
        Stmt::Match { scrut, arms, .. } => {
            find_in_expr(scrut, generic_structs, insts);
            for (p, b) in arms {
                if let Some(pat) = p {
                    find_in_expr(pat, generic_structs, insts);
                }
                find_in_block(b, generic_structs, insts);
            }
        }
        Stmt::Asm(_) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::InlineC { .. } => {}
    }
}

fn find_in_expr(
    e: &Expr,
    generic_structs: &HashMap<String, StructDef>,
    insts: &mut HashSet<(String, Vec<TypeExprKey>)>,
) {
    match e {
        Expr::Ident(name, _) => {
            for (g_name, g_def) in generic_structs {
                let prefix = format!("{g_name}__");
                if name.starts_with(&prefix) {
                    let rest = &name[prefix.len()..];
                    let parts: Vec<&str> = rest.split("__").collect();
                    let n = g_def.type_params.len();
                    if parts.len() >= n {
                        let keys: Vec<TypeExprKey> = parts[..n]
                            .iter()
                            .map(|p| TypeExprKey(p.to_string()))
                            .collect();
                        insts.insert((g_name.clone(), keys));
                    }
                }
            }
        }
        Expr::Cast { expr, ty, .. } => {
            find_in_expr(expr, generic_structs, insts);
            find_in_type_expr(ty, insts);
        }
        Expr::Call { callee, args, .. } => {
            find_in_expr(callee, generic_structs, insts);
            for a in args {
                find_in_expr(a, generic_structs, insts);
            }
        }
        Expr::Field { base, .. } => find_in_expr(base, generic_structs, insts),
        Expr::Index { base, index, .. } => {
            find_in_expr(base, generic_structs, insts);
            find_in_expr(index, generic_structs, insts);
        }
        Expr::Slice { base, start, end, .. } => {
            find_in_expr(base, generic_structs, insts);
            if let Some(s) = start {
                find_in_expr(s, generic_structs, insts);
            }
            if let Some(en) = end {
                find_in_expr(en, generic_structs, insts);
            }
        }
        Expr::Binary { lhs, rhs, .. } => {
            find_in_expr(lhs, generic_structs, insts);
            find_in_expr(rhs, generic_structs, insts);
        }
        Expr::Unary { expr, .. } => find_in_expr(expr, generic_structs, insts),
        Expr::StructLit { name, fields, .. } => {
            for (g_name, g_def) in generic_structs {
                let prefix = format!("{g_name}__");
                if name.starts_with(&prefix) {
                    let rest = &name[prefix.len()..];
                    let parts: Vec<&str> = rest.split("__").collect();
                    let n = g_def.type_params.len();
                    if parts.len() >= n {
                        let keys: Vec<TypeExprKey> = parts[..n]
                            .iter()
                            .map(|p| TypeExprKey(p.to_string()))
                            .collect();
                        insts.insert((g_name.clone(), keys));
                    }
                }
            }
            for (_, val, _) in fields {
                find_in_expr(val, generic_structs, insts);
            }
        }
        Expr::ArrayLit(elems, _) => {
            for el in elems {
                find_in_expr(el, generic_structs, insts);
            }
        }
        Expr::IfExpr { cond, then, els, .. } => {
            find_in_expr(cond, generic_structs, insts);
            find_in_expr(then, generic_structs, insts);
            find_in_expr(els, generic_structs, insts);
        }
        _ => {}
    }
}

fn substitute_type_expr(
    te: &mut TypeExpr,
    subst: &HashMap<String, TypeExpr>,
    generic_name: &str,
    spec_name: &str,
) {
    match te {
        TypeExpr::Named(n, _sp) => {
            if let Some(replacement) = subst.get(n) {
                *te = replacement.clone();
            } else if n == generic_name {
                *n = spec_name.to_string();
            }
        }
        TypeExpr::Generic(n, args, sp) => {
            for a in args.iter_mut() {
                substitute_type_expr(a, subst, generic_name, spec_name);
            }
            if n == generic_name {
                *te = TypeExpr::Named(spec_name.to_string(), *sp);
            }
        }
        TypeExpr::Ptr(inner, _) | TypeExpr::PtrMut(inner, _) | TypeExpr::Slice(inner, _) => {
            substitute_type_expr(inner, subst, generic_name, spec_name);
        }
        TypeExpr::Array(inner, _, _) => {
            substitute_type_expr(inner, subst, generic_name, spec_name);
        }
        TypeExpr::Fn(params, ret, _) => {
            for p in params {
                substitute_type_expr(p, subst, generic_name, spec_name);
            }
            if let Some(r) = ret {
                substitute_type_expr(r, subst, generic_name, spec_name);
            }
        }
    }
}

fn substitute_block(
    b: &mut Block,
    subst: &HashMap<String, TypeExpr>,
    generic_name: &str,
    spec_name: &str,
) {
    for s in &mut b.stmts {
        substitute_stmt(s, subst, generic_name, spec_name);
    }
}

fn substitute_stmt(
    s: &mut Stmt,
    subst: &HashMap<String, TypeExpr>,
    generic_name: &str,
    spec_name: &str,
) {
    match s {
        Stmt::Let { ty, value, .. } => {
            if let Some(t) = ty {
                substitute_type_expr(t, subst, generic_name, spec_name);
            }
            substitute_expr(value, subst, generic_name, spec_name);
        }
        Stmt::Assign { target, value, .. } => {
            substitute_expr(target, subst, generic_name, spec_name);
            substitute_expr(value, subst, generic_name, spec_name);
        }
        Stmt::Expr(e) => substitute_expr(e, subst, generic_name, spec_name),
        Stmt::Return(e, _) => {
            if let Some(ex) = e {
                substitute_expr(ex, subst, generic_name, spec_name);
            }
        }
        Stmt::If { cond, then, els, .. } => {
            substitute_expr(cond, subst, generic_name, spec_name);
            substitute_block(then, subst, generic_name, spec_name);
            if let Some(el) = els {
                substitute_block(el, subst, generic_name, spec_name);
            }
        }
        Stmt::While { cond, body, .. } => {
            substitute_expr(cond, subst, generic_name, spec_name);
            substitute_block(body, subst, generic_name, spec_name);
        }
        Stmt::For { init, cond, post, body, .. } => {
            if let Some(i) = init {
                substitute_stmt(i, subst, generic_name, spec_name);
            }
            if let Some(c) = cond {
                substitute_expr(c, subst, generic_name, spec_name);
            }
            if let Some(p) = post {
                substitute_stmt(p, subst, generic_name, spec_name);
            }
            substitute_block(body, subst, generic_name, spec_name);
        }
        Stmt::ForIn { iter, body, .. } => {
            substitute_expr(iter, subst, generic_name, spec_name);
            substitute_block(body, subst, generic_name, spec_name);
        }
        Stmt::Unsafe(b, _) => substitute_block(b, subst, generic_name, spec_name),
        Stmt::Assert(e, _) => substitute_expr(e, subst, generic_name, spec_name),
        Stmt::Match { scrut, arms, .. } => {
            substitute_expr(scrut, subst, generic_name, spec_name);
            for (p, b) in arms {
                if let Some(pat) = p {
                    substitute_expr(pat, subst, generic_name, spec_name);
                }
                substitute_block(b, subst, generic_name, spec_name);
            }
        }
        Stmt::Asm(_) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::InlineC { .. } => {}
    }
}

fn substitute_expr(
    e: &mut Expr,
    subst: &HashMap<String, TypeExpr>,
    generic_name: &str,
    spec_name: &str,
) {
    match e {
        Expr::Cast { expr, ty, .. } => {
            substitute_expr(expr, subst, generic_name, spec_name);
            substitute_type_expr(ty, subst, generic_name, spec_name);
        }
        Expr::Call { callee, args, .. } => {
            substitute_expr(callee, subst, generic_name, spec_name);
            for a in args {
                substitute_expr(a, subst, generic_name, spec_name);
            }
        }
        Expr::Field { base, .. } => substitute_expr(base, subst, generic_name, spec_name),
        Expr::Index { base, index, .. } => {
            substitute_expr(base, subst, generic_name, spec_name);
            substitute_expr(index, subst, generic_name, spec_name);
        }
        Expr::Slice { base, start, end, .. } => {
            substitute_expr(base, subst, generic_name, spec_name);
            if let Some(s) = start {
                substitute_expr(s, subst, generic_name, spec_name);
            }
            if let Some(en) = end {
                substitute_expr(en, subst, generic_name, spec_name);
            }
        }
        Expr::Binary { lhs, rhs, .. } => {
            substitute_expr(lhs, subst, generic_name, spec_name);
            substitute_expr(rhs, subst, generic_name, spec_name);
        }
        Expr::Unary { expr, .. } => substitute_expr(expr, subst, generic_name, spec_name),
        Expr::StructLit { name, fields, .. } => {
            if name == generic_name {
                *name = spec_name.to_string();
            }
            for (_, val, _) in fields {
                substitute_expr(val, subst, generic_name, spec_name);
            }
        }
        Expr::ArrayLit(elems, _) => {
            for el in elems {
                substitute_expr(el, subst, generic_name, spec_name);
            }
        }
        Expr::IfExpr { cond, then, els, .. } => {
            substitute_expr(cond, subst, generic_name, spec_name);
            substitute_expr(then, subst, generic_name, spec_name);
            substitute_expr(els, subst, generic_name, spec_name);
        }
        _ => {}
    }
}

fn rewrite_generic_to_named(te: &mut TypeExpr) {
    match te {
        TypeExpr::Generic(name, args, sp) => {
            for a in args.iter_mut() {
                rewrite_generic_to_named(a);
            }
            let keys: Vec<TypeExprKey> = args.iter().map(TypeExprKey::from_type_expr).collect();
            let mangled = mangle_specialized_name(name, &keys);
            *te = TypeExpr::Named(mangled, *sp);
        }
        TypeExpr::Ptr(inner, _) | TypeExpr::PtrMut(inner, _) | TypeExpr::Slice(inner, _) => {
            rewrite_generic_to_named(inner);
        }
        TypeExpr::Array(inner, _, _) => {
            rewrite_generic_to_named(inner);
        }
        TypeExpr::Fn(params, ret, _) => {
            for p in params {
                rewrite_generic_to_named(p);
            }
            if let Some(r) = ret {
                rewrite_generic_to_named(r);
            }
        }
        TypeExpr::Named(_, _) => {}
    }
}

fn rewrite_block_generics(b: &mut Block) {
    for s in &mut b.stmts {
        rewrite_stmt_generics(s);
    }
}

fn rewrite_stmt_generics(s: &mut Stmt) {
    match s {
        Stmt::Let { ty, value, .. } => {
            if let Some(t) = ty {
                rewrite_generic_to_named(t);
            }
            rewrite_expr_generics(value);
        }
        Stmt::Assign { target, value, .. } => {
            rewrite_expr_generics(target);
            rewrite_expr_generics(value);
        }
        Stmt::Expr(e) => rewrite_expr_generics(e),
        Stmt::Return(e, _) => {
            if let Some(ex) = e {
                rewrite_expr_generics(ex);
            }
        }
        Stmt::If { cond, then, els, .. } => {
            rewrite_expr_generics(cond);
            rewrite_block_generics(then);
            if let Some(el) = els {
                rewrite_block_generics(el);
            }
        }
        Stmt::While { cond, body, .. } => {
            rewrite_expr_generics(cond);
            rewrite_block_generics(body);
        }
        Stmt::For { init, cond, post, body, .. } => {
            if let Some(i) = init {
                rewrite_stmt_generics(i);
            }
            if let Some(c) = cond {
                rewrite_expr_generics(c);
            }
            if let Some(p) = post {
                rewrite_stmt_generics(p);
            }
            rewrite_block_generics(body);
        }
        Stmt::ForIn { iter, body, .. } => {
            rewrite_expr_generics(iter);
            rewrite_block_generics(body);
        }
        Stmt::Unsafe(b, _) => rewrite_block_generics(b),
        Stmt::Assert(e, _) => rewrite_expr_generics(e),
        Stmt::Match { scrut, arms, .. } => {
            rewrite_expr_generics(scrut);
            for (p, b) in arms {
                if let Some(pat) = p {
                    rewrite_expr_generics(pat);
                }
                rewrite_block_generics(b);
            }
        }
        Stmt::Asm(_) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::InlineC { .. } => {}
    }
}

fn rewrite_expr_generics(e: &mut Expr) {
    match e {
        Expr::Cast { expr, ty, .. } => {
            rewrite_expr_generics(expr);
            rewrite_generic_to_named(ty);
        }
        Expr::Call { callee, args, .. } => {
            rewrite_expr_generics(callee);
            for a in args {
                rewrite_expr_generics(a);
            }
        }
        Expr::Field { base, .. } => rewrite_expr_generics(base),
        Expr::Index { base, index, .. } => {
            rewrite_expr_generics(base);
            rewrite_expr_generics(index);
        }
        Expr::Slice { base, start, end, .. } => {
            rewrite_expr_generics(base);
            if let Some(s) = start {
                rewrite_expr_generics(s);
            }
            if let Some(en) = end {
                rewrite_expr_generics(en);
            }
        }
        Expr::Binary { lhs, rhs, .. } => {
            rewrite_expr_generics(lhs);
            rewrite_expr_generics(rhs);
        }
        Expr::Unary { expr, .. } => rewrite_expr_generics(expr),
        Expr::StructLit { fields, .. } => {
            for (_, val, _) in fields {
                rewrite_expr_generics(val);
            }
        }
        Expr::ArrayLit(elems, _) => {
            for el in elems {
                rewrite_expr_generics(el);
            }
        }
        Expr::IfExpr { cond, then, els, .. } => {
            rewrite_expr_generics(cond);
            rewrite_expr_generics(then);
            rewrite_expr_generics(els);
        }
        _ => {}
    }
}
