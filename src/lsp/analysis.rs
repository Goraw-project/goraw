use crate::ast::*;
use crate::diag::{Diags, Span};
use crate::lexer::{Lexer, Token};
use crate::lsp::protocol::*;
use crate::parser::Parser;
use crate::types::{collect, TyCtx};

pub struct DocumentAnalysis {
    pub uri: String,
    pub text: String,
    pub line_offsets: Vec<usize>,
    pub program: Program,
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
    pub ty_ctx: TyCtx,
}

impl DocumentAnalysis {
    pub fn new(uri: String, text: String) -> Self {
        let mut line_offsets = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_offsets.push(i + 1);
            }
        }

        let mut diags = Diags::new(&uri, &text);
        let mut lexer = Lexer::new(&text);
        let tokens = lexer.tokenize(&mut diags);

        let mut parser = Parser::new(tokens.clone(), &text, &mut diags);
        let mut program = parser.parse_program();

        // Мономорфизация (безопасно, без паники)
        crate::mono::monomorphize(&mut program);

        // Тайпчек / сбор сигнатур
        let mut type_diags = Vec::new();
        let ty_ctx = collect(&program.structs, &program.enums, &program.fns, &mut type_diags);
        diags.items.extend(type_diags);

        // Полный семантический анализ выражений и тел функций
        let cg = crate::codegen::Codegen::new(&ty_ctx, &mut diags);
        let _ = cg.emit_module(&program);

        // Преобразование ошибок Goraw в LSP Diagnostics
        let mut lsp_diags = Vec::new();
        for d in &diags.items {
            let range = span_to_range(&line_offsets, &text, d.span);
            let mut message = d.message.clone();
            if let Some(h) = &d.hint {
                message.push_str("\nПодсказка: ");
                message.push_str(h);
            }
            let severity = match d.severity {
                crate::diag::Severity::Error => 1,
                crate::diag::Severity::Warning => 2,
            };
            lsp_diags.push(Diagnostic {
                range,
                severity,
                code: Some(d.code.to_string()),
                source: Some("goraw".to_string()),
                message,
            });
        }

        Self {
            uri,
            text,
            line_offsets,
            program,
            tokens,
            diagnostics: lsp_diags,
            ty_ctx,
        }
    }

    pub fn pos_to_offset(&self, pos: Position) -> usize {
        let line = (pos.line as usize).min(self.line_offsets.len().saturating_sub(1));
        let line_start = self.line_offsets.get(line).copied().unwrap_or(0);
        let line_end = self
            .line_offsets
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len());
        (line_start + pos.character as usize).min(line_end)
    }

    pub fn offset_to_pos(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = match self.line_offsets.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_offsets.get(line).copied().unwrap_or(0);
        let character = offset.saturating_sub(line_start);
        Position {
            line: line as u32,
            character: character as u32,
        }
    }

    pub fn get_word_at(&self, pos: Position) -> Option<(String, Range)> {
        let offset = self.pos_to_offset(pos);
        if self.text.is_empty() {
            return None;
        }

        let bytes = self.text.as_bytes();
        let idx = offset.min(bytes.len().saturating_sub(1));

        // Если курсор на пробеле/знаке, проверим символ слева
        let test_idx = if idx > 0 && !is_ident_char(bytes[idx] as char) && is_ident_char(bytes[idx - 1] as char) {
            idx - 1
        } else {
            idx
        };

        if !is_ident_char(bytes[test_idx] as char) {
            return None;
        }

        let mut start = test_idx;
        while start > 0 && is_ident_char(bytes[start - 1] as char) {
            start -= 1;
        }
        let mut end = test_idx;
        while end < bytes.len() && is_ident_char(bytes[end] as char) {
            end += 1;
        }

        let word = self.text[start..end].to_string();
        let range = Range {
            start: self.offset_to_pos(start),
            end: self.offset_to_pos(end),
        };
        Some((word, range))
    }

    pub fn hover(&self, pos: Position) -> Option<Hover> {
        let (word, range) = self.get_word_at(pos)?;

        // 1. Встроенные ключевые слова и типы
        if let Some(builtin_doc) = get_builtin_hover(&word) {
            return Some(Hover {
                contents: MarkupContent {
                    kind: "markdown".to_string(),
                    value: builtin_doc,
                },
                range: Some(range),
            });
        }

        // 2. Функции и методы
        for f in &self.program.fns {
            if f.name == word
                || f.name.ends_with(&format!("::{word}"))
                || f.name.ends_with(&format!("__{word}"))
            {
                let sig = format_fn_signature(f);
                let mut doc = format!("```goraw\n{sig}\n```");
                if f.name.contains("::") || f.name.contains("__") {
                    let parts: Vec<&str> = if f.name.contains("::") {
                        f.name.split("::").collect()
                    } else {
                        f.name.split("__").collect()
                    };
                    doc.push_str(&format!("\n\n*Метод структуры `{}`*", parts[0]));
                }
                if f.is_unsafe {
                    doc.push_str("\n\n⚠️ **unsafe**: требует явного блока `unsafe { ... }`");
                }
                return Some(Hover {
                    contents: MarkupContent {
                        kind: "markdown".to_string(),
                        value: doc,
                    },
                    range: Some(range),
                });
            }
        }

        // 3. Структуры
        for s in &self.program.structs {
            if s.name == word {
                let mut doc = format!("```goraw\nstruct {} {{\n", s.name);
                for field in &s.fields {
                    doc.push_str(&format!("    {}: {},\n", field.name, format_type_expr(&field.ty)));
                }
                doc.push_str("}\n```");
                return Some(Hover {
                    contents: MarkupContent {
                        kind: "markdown".to_string(),
                        value: doc,
                    },
                    range: Some(range),
                });
            }
        }

        // 4. Поля структуры (если курсор на поле внутри метода или литерала)
        for s in &self.program.structs {
            for field in &s.fields {
                if field.name == word {
                    let doc = format!("```goraw\n{}.{}: {}\n```", s.name, field.name, format_type_expr(&field.ty));
                    return Some(Hover {
                        contents: MarkupContent {
                            kind: "markdown".to_string(),
                            value: doc,
                        },
                        range: Some(range),
                    });
                }
            }
        }

        // 5. Локальные переменные и параметры
        let offset = self.pos_to_offset(pos);
        if let Some(var_doc) = self.find_local_var(offset, &word) {
            return Some(Hover {
                contents: MarkupContent {
                    kind: "markdown".to_string(),
                    value: var_doc,
                },
                range: Some(range),
            });
        }

        None
    }

    pub fn definition(&self, pos: Position) -> Option<Location> {
        let (word, _) = self.get_word_at(pos)?;

        // 1. Поиск среди функций и методов
        for f in &self.program.fns {
            if f.name == word
                || f.name.ends_with(&format!("::{word}"))
                || f.name.ends_with(&format!("__{word}"))
            {
                let range = span_to_range(&self.line_offsets, &self.text, f.span);
                return Some(Location {
                    uri: self.uri.clone(),
                    range,
                });
            }
        }

        // 2. Поиск среди структур
        for s in &self.program.structs {
            if s.name == word {
                let range = span_to_range(&self.line_offsets, &self.text, s.span);
                return Some(Location {
                    uri: self.uri.clone(),
                    range,
                });
            }
        }

        // 3. Поиск среди полей структур
        for s in &self.program.structs {
            for f in &s.fields {
                if f.name == word {
                    let range = span_to_range(&self.line_offsets, &self.text, f.span);
                    return Some(Location {
                        uri: self.uri.clone(),
                        range,
                    });
                }
            }
        }

        // 4. Локальные переменные
        let offset = self.pos_to_offset(pos);
        if let Some(span) = self.find_local_var_span(offset, &word) {
            let range = span_to_range(&self.line_offsets, &self.text, span);
            return Some(Location {
                uri: self.uri.clone(),
                range,
            });
        }

        None
    }

    pub fn completion(&self, pos: Position) -> Vec<CompletionItem> {
        let line_idx = pos.line as usize;
        let line_start = self.line_offsets.get(line_idx).copied().unwrap_or(0);
        let line_end = self
            .line_offsets
            .get(line_idx + 1)
            .copied()
            .unwrap_or(self.text.len());
        let current_offset = (line_start + pos.character as usize).min(line_end);
        let line_prefix = &self.text[line_start..current_offset];

        // Случай A: автодополнение точки `s.`
        if let Some(dot_idx) = line_prefix.rfind('.') {
            let before_dot = line_prefix[..dot_idx].trim_end();
            if let Some(var_name) = extract_identifier_suffix(before_dot) {
                return self.get_dot_completions(current_offset, var_name);
            }
        }

        // Случай B: автодополнение пространства имён `Type::`
        if let Some(colon_idx) = line_prefix.rfind("::") {
            let before_colon = line_prefix[..colon_idx].trim_end();
            if let Some(type_name) = extract_identifier_suffix(before_colon) {
                return self.get_namespace_completions(type_name);
            }
        }

        // Случай C: общее автодополнение (ключевые слова, типы, встроенные функции, символы файла)
        self.get_general_completions()
    }

    fn get_dot_completions(&self, offset: usize, var_name: &str) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        // Определяем тип переменной
        let mut struct_type: Option<String> = None;
        let mut is_str = false;
        let mut is_slice = false;

        if var_name == "self" {
            // Ищем функцию, содержащую текущее смещение
            for f in &self.program.fns {
                if f.span.lo.offset <= offset && offset <= f.span.hi.offset {
                    if let Some(pos) = f.name.find("::") {
                        let struct_name = &f.name[..pos];
                        // отсекаем дженерики если есть: Box<T> -> Box
                        let base_struct = struct_name.split('<').next().unwrap_or(struct_name);
                        struct_type = Some(base_struct.to_string());
                    }
                }
            }
        } else {
            // Ищем локальную переменную в теле функции
            for f in &self.program.fns {
                if f.span.lo.offset <= offset && offset <= f.span.hi.offset {
                    for p in &f.params {
                        if p.name == var_name {
                            if let TypeExpr::Named(n, _) = &p.ty {
                                struct_type = Some(n.clone());
                            } else if let TypeExpr::Slice(..) = &p.ty {
                                is_slice = true;
                            }
                        }
                    }
                    if let Some(body) = &f.body {
                        find_var_type_in_block(body, var_name, &mut struct_type, &mut is_str, &mut is_slice);
                    }
                }
            }
        }

        if is_str {
            items.push(CompletionItem {
                label: "len()".to_string(),
                kind: CompletionItemKind::METHOD,
                detail: Some("fn len(self) -> i64".to_string()),
                documentation: Some(MarkupContent {
                    kind: "markdown".to_string(),
                    value: "Возвращает длину строки в байтах.".to_string(),
                }),
                insert_text: Some("len()".to_string()),
            });
            items.push(CompletionItem {
                label: "starts_with(prefix)".to_string(),
                kind: CompletionItemKind::METHOD,
                detail: Some("fn starts_with(self, prefix: str) -> bool".to_string()),
                documentation: None,
                insert_text: Some("starts_with(${1:prefix})".to_string()),
            });
            items.push(CompletionItem {
                label: "ends_with(suffix)".to_string(),
                kind: CompletionItemKind::METHOD,
                detail: Some("fn ends_with(self, suffix: str) -> bool".to_string()),
                documentation: None,
                insert_text: Some("ends_with(${1:suffix})".to_string()),
            });
            items.push(CompletionItem {
                label: "is_empty()".to_string(),
                kind: CompletionItemKind::METHOD,
                detail: Some("fn is_empty(self) -> bool".to_string()),
                documentation: None,
                insert_text: Some("is_empty()".to_string()),
            });
            items.push(CompletionItem {
                label: "clone()".to_string(),
                kind: CompletionItemKind::METHOD,
                detail: Some("fn clone(self) -> str".to_string()),
                documentation: None,
                insert_text: Some("clone()".to_string()),
            });
            return items;
        }

        if is_slice {
            items.push(CompletionItem {
                label: "ptr".to_string(),
                kind: CompletionItemKind::FIELD,
                detail: Some("*mut T".to_string()),
                documentation: Some(MarkupContent {
                    kind: "markdown".to_string(),
                    value: "Указатель на первый элемент среза.".to_string(),
                }),
                insert_text: Some("ptr".to_string()),
            });
            items.push(CompletionItem {
                label: "len".to_string(),
                kind: CompletionItemKind::FIELD,
                detail: Some("i64".to_string()),
                documentation: Some(MarkupContent {
                    kind: "markdown".to_string(),
                    value: "Количество элементов в срезе.".to_string(),
                }),
                insert_text: Some("len".to_string()),
            });
            return items;
        }

        // Если нашли структуру — выдаем поля и методы
        if let Some(st) = struct_type {
            // 1. Поля структуры
            if let Some(s) = self.program.structs.iter().find(|s| s.name == st) {
                for f in &s.fields {
                    items.push(CompletionItem {
                        label: f.name.clone(),
                        kind: CompletionItemKind::FIELD,
                        detail: Some(format_type_expr(&f.ty)),
                        documentation: None,
                        insert_text: Some(f.name.clone()),
                    });
                }
            }
            // 2. Методы структуры: `fn Struct::method` (или Struct__method)
            let prefix_colon = format!("{st}::");
            let prefix_under = format!("{st}__");
            for f in &self.program.fns {
                let method_name = if f.name.starts_with(&prefix_colon) {
                    Some(&f.name[prefix_colon.len()..])
                } else if f.name.starts_with(&prefix_under) {
                    Some(&f.name[prefix_under.len()..])
                } else {
                    None
                };
                if let Some(m) = method_name {
                    let sig = format_fn_signature(f);
                    let insert = if f.params.len() > 1 {
                        format!("{m}($0)")
                    } else {
                        format!("{m}()")
                    };
                    items.push(CompletionItem {
                        label: format!("{m}()"),
                        kind: CompletionItemKind::METHOD,
                        detail: Some(sig),
                        documentation: None,
                        insert_text: Some(insert),
                    });
                }
            }
        }

        items
    }

    fn get_namespace_completions(&self, type_name: &str) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        let prefix_colon = format!("{type_name}::");
        let prefix_under = format!("{type_name}__");
        for f in &self.program.fns {
            let method_name = if f.name.starts_with(&prefix_colon) {
                Some(&f.name[prefix_colon.len()..])
            } else if f.name.starts_with(&prefix_under) {
                Some(&f.name[prefix_under.len()..])
            } else {
                None
            };
            if let Some(m) = method_name {
                items.push(CompletionItem {
                    label: m.to_string(),
                    kind: CompletionItemKind::METHOD,
                    detail: Some(format_fn_signature(f)),
                    documentation: None,
                    insert_text: Some(format!("{m}($0)")),
                });
            }
        }
        items
    }

    fn get_general_completions(&self) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        // 1. Ключевые слова
        let keywords = [
            ("fn", "fn name(args) -> Ret { ... }"),
            ("struct", "struct Name { ... }"),
            ("enum", "enum Name { ... }"),
            ("let", "let [mut] name: Type = expr;"),
            ("mut", "mut name"),
            ("const", "const NAME: Type = expr;"),
            ("static", "static NAME: Type = expr;"),
            ("if", "if cond { ... }"),
            ("else", "else { ... }"),
            ("while", "while cond { ... }"),
            ("for", "for item in slice { ... }"),
            ("in", "in collection"),
            ("return", "return expr;"),
            ("unsafe", "unsafe { ... }"),
            ("test", "test \"name\" { assert ...; }"),
            ("shadow", "shadow fn_name { assert ...; }"),
            ("as", "expr as Type"),
            ("import", "import \"path.gw\";"),
            ("true", "bool true"),
            ("false", "bool false"),
            ("null", "null pointer"),
        ];
        for (kw, detail) in keywords {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: CompletionItemKind::KEYWORD,
                detail: Some(detail.to_string()),
                documentation: None,
                insert_text: Some(kw.to_string()),
            });
        }

        // 2. Типы
        let types = [
            ("i8", "8-битное целое со знаком"),
            ("i16", "16-битное целое со знаком"),
            ("i32", "32-битное целое со знаком"),
            ("i64", "64-битное целое со знаком"),
            ("u8", "8-битное целое без знака"),
            ("u16", "16-битное целое без знака"),
            ("u32", "32-битное целое без знака"),
            ("u64", "64-битное целое без знака"),
            ("f32", "32-битное число с плавающей точкой"),
            ("f64", "64-битное число с плавающей точкой"),
            ("bool", "логический тип (true/false)"),
            ("void", "тип отсутствия значения"),
            ("str", "строковый тип (срез байтов []u8)"),
            ("Vec", "Generic динамический массив Vec<T>"),
            ("HashMap", "Generic хеш-таблица HashMap<K, V>"),
            ("Result", "Result<T, E> { Ok(T), Err(E) }"),
            ("Option", "Option<T> { Some(T), None }"),
            ("Box", "Умный указатель на куче с RAII Box<T>"),
        ];
        for (t, doc) in types {
            items.push(CompletionItem {
                label: t.to_string(),
                kind: CompletionItemKind::STRUCT,
                detail: Some(format!("тип {t}")),
                documentation: Some(MarkupContent {
                    kind: "markdown".to_string(),
                    value: doc.to_string(),
                }),
                insert_text: Some(t.to_string()),
            });
        }

        // 3. Встроенные функции
        let builtins = [
            ("sizeof", "sizeof(T) -> i64", "Размер типа в байтах"),
            ("zeroed", "zeroed() -> T", "Создаёт структуру с нулевыми байтами"),
            ("alloc", "alloc(bytes: i64) -> *mut u8", "Выделяет память в куче"),
            ("free", "free(ptr: *mut u8)", "Освобождает блок памяти"),
            ("realloc", "realloc(ptr, bytes) -> *mut u8", "Перевыделяет блок памяти"),
            ("mem_copy", "mem_copy(dst, src, bytes)", "Копирует байты памяти"),
            ("mem_set", "mem_set(dst, byte, count)", "Заполняет память байтом"),
            ("make_slice", "make_slice(ptr, len) -> []T", "Создаёт срез из указателя и длины"),
            ("print", "print(\"fmt\", ...)", "Форматированный вывод в stdout"),
            ("println", "println(\"fmt\", ...)", "Форматированный вывод со сносом строки"),
            ("panic", "panic(\"msg\")", "Аварийное завершение программы"),
            ("sqrt", "sqrt(x: f64) -> f64", "Квадратный корень"),
            ("sin", "sin(x: f64) -> f64", "Синус угла в радианах"),
            ("cos", "cos(x: f64) -> f64", "Косинус угла в радианах"),
            ("exp", "exp(x: f64) -> f64", "Экспонента e^x"),
            ("log", "log(x: f64) -> f64", "Натуральный логарифм"),
            ("floor", "floor(x: f64) -> f64", "Округление вниз"),
            ("ceil", "ceil(x: f64) -> f64", "Округление вверх"),
            ("round", "round(x: f64) -> f64", "Округление к ближайшему целому"),
            ("abs", "abs(x: f64) -> f64", "Абсолютное значение"),
            ("min", "min(a, b)", "Минимум из двух значений"),
            ("max", "max(a, b)", "Максимум из двух значений"),
            ("clamp", "clamp(val, low, high)", "Ограничение значения в диапазоне"),
        ];
        for (b, sig, doc) in builtins {
            items.push(CompletionItem {
                label: b.to_string(),
                kind: CompletionItemKind::FUNCTION,
                detail: Some(sig.to_string()),
                documentation: Some(MarkupContent {
                    kind: "markdown".to_string(),
                    value: doc.to_string(),
                }),
                insert_text: Some(format!("{b}($0)")),
            });
        }

        // 4. Функции пользователя
        for f in &self.program.fns {
            if !f.name.contains("::") {
                items.push(CompletionItem {
                    label: f.name.clone(),
                    kind: CompletionItemKind::FUNCTION,
                    detail: Some(format_fn_signature(f)),
                    documentation: None,
                    insert_text: Some(format!("{}($0)", f.name)),
                });
            }
        }

        // 5. Структуры пользователя
        for s in &self.program.structs {
            items.push(CompletionItem {
                label: s.name.clone(),
                kind: CompletionItemKind::STRUCT,
                detail: Some(format!("struct {}", s.name)),
                documentation: None,
                insert_text: Some(s.name.clone()),
            });
        }

        items
    }

    fn find_local_var(&self, offset: usize, name: &str) -> Option<String> {
        for f in &self.program.fns {
            if f.span.lo.offset <= offset && offset <= f.span.hi.offset {
                for p in &f.params {
                    if p.name == name {
                        return Some(format!("```goraw\n(параметр) {}: {}\n```", p.name, format_type_expr(&p.ty)));
                    }
                }
                if let Some(body) = &f.body {
                    if let Some(s) = find_let_in_block(body, name) {
                        return Some(s);
                    }
                }
            }
        }
        None
    }

    fn find_local_var_span(&self, offset: usize, name: &str) -> Option<Span> {
        for f in &self.program.fns {
            if f.span.lo.offset <= offset && offset <= f.span.hi.offset {
                for p in &f.params {
                    if p.name == name {
                        return Some(p.span);
                    }
                }
                if let Some(body) = &f.body {
                    if let Some(span) = find_let_span_in_block(body, name) {
                        return Some(span);
                    }
                }
            }
        }
        None
    }
}

fn span_to_range(line_offsets: &[usize], text: &str, span: Span) -> Range {
    if span.lo.line > 0 && span.hi.line > 0 {
        let start_line = (span.lo.line - 1) as u32;
        let start_col = span.lo.col.saturating_sub(1);
        let end_line = (span.hi.line - 1) as u32;
        let mut end_col = span.hi.col.saturating_sub(1);
        if start_line == end_line && end_col <= start_col {
            end_col = start_col + 1;
        }
        return Range {
            start: Position {
                line: start_line,
                character: start_col,
            },
            end: Position {
                line: end_line,
                character: end_col,
            },
        };
    }

    let start_offset = span.lo.offset.min(text.len());
    let end_offset = span.hi.offset.min(text.len());

    let start_line = match line_offsets.binary_search(&start_offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    };
    let start_col = start_offset.saturating_sub(line_offsets.get(start_line).copied().unwrap_or(0));

    let end_line = match line_offsets.binary_search(&end_offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    };
    let end_col = end_offset.saturating_sub(line_offsets.get(end_line).copied().unwrap_or(0));

    Range {
        start: Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: Position {
            line: end_line as u32,
            character: (if end_col <= start_col && start_line == end_line { start_col + 1 } else { end_col }) as u32,
        },
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn extract_identifier_suffix(s: &str) -> Option<&str> {
    let end = s.len();
    if end == 0 {
        return None;
    }
    let mut start = end;
    let bytes = s.as_bytes();
    while start > 0 && is_ident_char(bytes[start - 1] as char) {
        start -= 1;
    }
    if start == end {
        None
    } else {
        Some(&s[start..end])
    }
}

fn format_type_expr(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(n, _) => n.clone(),
        TypeExpr::Generic(n, args, _) => {
            let formatted_args: Vec<String> = args.iter().map(format_type_expr).collect();
            format!("{n}<{}>", formatted_args.join(", "))
        }
        TypeExpr::Ptr(inner, _) => format!("*{}", format_type_expr(inner)),
        TypeExpr::PtrMut(inner, _) => format!("*mut {}", format_type_expr(inner)),
        TypeExpr::Fn(params, ret, _) => {
            let ps: Vec<String> = params.iter().map(format_type_expr).collect();
            let r = ret.as_ref().map(|x| format!(" -> {}", format_type_expr(x))).unwrap_or_default();
            format!("fn({}){}", ps.join(", "), r)
        }
        TypeExpr::Slice(inner, _) => format!("[]{}", format_type_expr(inner)),
        TypeExpr::Array(inner, size, _) => format!("[{}]{}", size, format_type_expr(inner)),
    }
}

fn format_fn_signature(f: &FnDef) -> String {
    let mut s = String::new();
    if f.is_unsafe {
        s.push_str("unsafe ");
    }
    if f.is_extern {
        s.push_str("extern ");
    }
    s.push_str("fn ");
    s.push_str(&f.name.replace("__", "::"));
    s.push('(');
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&p.name);
        s.push_str(": ");
        s.push_str(&format_type_expr(&p.ty));
    }
    if f.variadic {
        if !f.params.is_empty() {
            s.push_str(", ");
        }
        s.push_str("...");
    }
    s.push(')');
    if let Some(r) = &f.ret {
        s.push_str(" -> ");
        s.push_str(&format_type_expr(r));
    }
    s
}

fn find_var_type_in_block(
    block: &Block,
    var_name: &str,
    struct_type: &mut Option<String>,
    is_str: &mut bool,
    is_slice: &mut bool,
) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { name, ty, value, .. } if name == var_name => {
                if let Some(t) = ty {
                    match t {
                        TypeExpr::Named(n, _) => {
                            if n == "str" {
                                *is_str = true;
                            } else {
                                *struct_type = Some(n.clone());
                            }
                        }
                        TypeExpr::Slice(..) => {
                            *is_slice = true;
                        }
                        _ => {}
                    }
                } else {
                    // Вывод из литерала структуры: `let s = Stack { ... };`
                    if let Expr::StructLit { name: s_name, .. } = value {
                        *struct_type = Some(s_name.clone());
                    }
                }
            }
            Stmt::If { then, els, .. } => {
                find_var_type_in_block(then, var_name, struct_type, is_str, is_slice);
                if let Some(e) = els {
                    find_var_type_in_block(e, var_name, struct_type, is_str, is_slice);
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                find_var_type_in_block(body, var_name, struct_type, is_str, is_slice);
            }
            Stmt::Unsafe(b, _) => {
                find_var_type_in_block(b, var_name, struct_type, is_str, is_slice);
            }
            _ => {}
        }
    }
}

fn find_let_in_block(block: &Block, name: &str) -> Option<String> {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let {
                name: var_name,
                mutable,
                ty,
                ..
            } if var_name == name => {
                let m = if *mutable { "mut " } else { "" };
                let t_str = ty
                    .as_ref()
                    .map(|t| format!(": {}", format_type_expr(t)))
                    .unwrap_or_default();
                return Some(format!("```goraw\nlet {m}{name}{t_str}\n```"));
            }
            Stmt::If { then, els, .. } => {
                if let Some(res) = find_let_in_block(then, name) {
                    return Some(res);
                }
                if let Some(e) = els {
                    if let Some(res) = find_let_in_block(e, name) {
                        return Some(res);
                    }
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                if let Some(res) = find_let_in_block(body, name) {
                    return Some(res);
                }
            }
            Stmt::Unsafe(b, _) => {
                if let Some(res) = find_let_in_block(b, name) {
                    return Some(res);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_let_span_in_block(block: &Block, name: &str) -> Option<Span> {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let {
                name: var_name, span, ..
            } if var_name == name => {
                return Some(*span);
            }
            Stmt::If { then, els, .. } => {
                if let Some(res) = find_let_span_in_block(then, name) {
                    return Some(res);
                }
                if let Some(e) = els {
                    if let Some(res) = find_let_span_in_block(e, name) {
                        return Some(res);
                    }
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                if let Some(res) = find_let_span_in_block(body, name) {
                    return Some(res);
                }
            }
            Stmt::Unsafe(b, _) => {
                if let Some(res) = find_let_span_in_block(b, name) {
                    return Some(res);
                }
            }
            _ => {}
        }
    }
    None
}

fn get_builtin_hover(word: &str) -> Option<String> {
    match word {
        "sizeof" => Some("### `sizeof(T) -> i64`\nВозвращает размер типа в байтах в компайл-тайме.".into()),
        "zeroed" => Some("### `zeroed() -> T`\nСоздаёт структуру или значение типа `T`, заполненное нулями.".into()),
        "alloc" => Some("### `alloc(bytes: i64) -> *mut u8`\nВыделяет неинициализированный блок памяти в куче.".into()),
        "free" => Some("### `free(ptr: *mut u8)`\nОсвобождает ранее выделенный блок памяти в куче.".into()),
        "realloc" => Some("### `realloc(ptr: *mut u8, new_size: i64) -> *mut u8`\nИзменяет размер блока памяти.".into()),
        "mem_copy" => Some("### `mem_copy(dst: *mut u8, src: *u8, count: i64)`\nКопирует блок байтов из `src` в `dst`.".into()),
        "mem_set" => Some("### `mem_set(dst: *mut u8, val: u8, count: i64)`\nЗаполняет память указанным байтом.".into()),
        "make_slice" => Some("### `make_slice(ptr: *mut T, len: i64) -> []T`\nСоздаёт срез `[]T` поверх существующего указателя.".into()),
        "print" => Some("### `print(fmt: str, ...)`\nФорматированный вывод в стандартный поток вывода.".into()),
        "println" => Some("### `println(fmt: str, ...)`\nФорматированный вывод с переводом строки в конце.".into()),
        "panic" => Some("### `panic(msg: str)`\nАварийно завершает работу программы с выводом ошибки.".into()),
        "unsafe" => Some("### `unsafe { ... }`\nБлок разрешённых низкоуровневых операций (разыменование сырых указателей, вызов unsafe-функций).".into()),
        "test" => Some("### `test \"name\" { ... }`\nВстроенный тестовый блок. Компилируется и запускается через `goraw -t`.".into()),
        "shadow" => Some("### `shadow fn_name { ... }`\nОбязательный тест-тень для функции. Проверяется строгим режимом `--shadow=strict`.".into()),
        _ => None,
    }
}
