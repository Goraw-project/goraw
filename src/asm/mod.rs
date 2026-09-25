//! `gorawas` — нативный ассемблер Goraw-asm (собственный Intel-диалект).
//!
//! Clean-room: мы не копируем NASM/MASM. «Наше» — фронтенд (лексер/парсер
//! диалекта, директивы данных, секции, резолв символов) и запись объектника.
//! Кодирование инструкций (ModRM/REX/VEX/EVEX) делегируется проверенному `iced-x86`,
//! формат COFF — крейту `object`. Ошибки идут через общий `crate::diag`, поэтому у
//! ассемблера — та же LLM-JSON диагностика, что и у компилятора.
//!
//! Поддержаны:
//! - Секции: `.text`, `.data`, `.rdata` (`.rodata`)
//! - Директивы: `global`, `extern`, `default rel`
//! - Данные: `db`, `dw`, `dd`, `dq` (со строками и escape-последовательностями)
//! - Операнды: r64/r32, imm (dec/hex/bin), память `[base + index*scale + disp]`,
//!   RIP-relative `[rip + sym]`, `[rel sym]` и `[sym]`
//! - Ветвления и вызовы: `jmp`, `jcc`, `loop`, `call` (как внутренние, так и внешние релокации COFF)

use crate::diag::{Diagnostic, Diags, Pos, Span};
use iced_x86::code_asm::*;
use iced_x86::BlockEncoderOptions;
use std::collections::HashMap;

use object::write::{Object, Symbol, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, RelocationEncoding, RelocationFlags, RelocationKind,
    SectionKind, SymbolFlags, SymbolKind, SymbolScope,
};

/// Результат сборки: байты COFF-объектника (если не было ошибок).
pub fn assemble(file: &str, src: &str) -> (Option<Vec<u8>>, Diags) {
    let mut diags = Diags::new(file, src);
    let program = parse(src, &mut diags);
    if diags.has_errors() {
        return (None, diags);
    }
    match encode(&program, &mut diags) {
        Some(obj) if !diags.has_errors() => (Some(obj), diags),
        _ => (None, diags),
    }
}

// ---------- разбор диалекта ----------

#[derive(Debug)]
enum Item {
    Label(String),
    Global(String),
    Extern(String),
    Section(String),
    Insn { mnem: String, ops: Vec<Operand>, span: Span },
    #[allow(dead_code)]
    Data { kind: DataKind, bytes: Vec<u8>, span: Span },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Db,
    Dw,
    Dd,
    Dq,
}

#[derive(Debug, Clone)]
enum Operand {
    R64(AsmRegister64),
    R32(AsmRegister32),
    Imm(i64),
    Mem(AsmMemoryOperand),
    RipRel(String),
    Label(String),
}

struct Program {
    items: Vec<Item>,
}

/// Разбор построчно: `;` — комментарий, `name:` — метка, директивы или инструкции.
fn parse(src: &str, diags: &mut Diags) -> Program {
    let mut items = Vec::new();

    for (i, raw) in src.lines().enumerate() {
        let line_no = (i + 1) as u32;
        // отрезаем комментарий
        let line = match raw.find(';') {
            Some(p) => &raw[..p],
            None => raw,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let span = Span::new(
            Pos { offset: 0, line: line_no, col: 1 },
            Pos { offset: 0, line: line_no, col: raw.len() as u32 + 1 },
        );

        // Метка: может быть отдельно (`name:`) или перед инструкцией/директивой (`name: db "...", 0`)
        let (maybe_lbl, rest_line) = if let Some(idx) = line.find(':') {
            let potential_lbl = line[..idx].trim();
            if is_ident(potential_lbl) {
                (Some(potential_lbl.to_string()), line[idx + 1..].trim())
            } else {
                (None, line)
            }
        } else {
            (None, line)
        };

        if let Some(lbl) = maybe_lbl {
            items.push(Item::Label(lbl));
        }

        if rest_line.is_empty() {
            continue;
        }

        // директива или инструкция
        let (head, rest) = split_first(rest_line);
        let head_lower = head.to_lowercase();
        match head_lower.as_str() {
            "section" | "segment" => items.push(Item::Section(rest.trim().to_string())),
            "global" | "globl" => {
                for g in rest.split(',') {
                    let g = g.trim();
                    if !g.is_empty() {
                        items.push(Item::Global(g.to_string()));
                    }
                }
            }
            "extern" | "extrn" => {
                for ext in rest.split(',') {
                    let ext = ext.trim();
                    if !ext.is_empty() {
                        items.push(Item::Extern(ext.to_string()));
                    }
                }
            }
            "default" => {
                // например `default rel` — поддерживаем без ошибок
            }
            "db" => {
                if let Some(bytes) = parse_data_operands(rest, DataKind::Db, span, diags) {
                    items.push(Item::Data { kind: DataKind::Db, bytes, span });
                }
            }
            "dw" => {
                if let Some(bytes) = parse_data_operands(rest, DataKind::Dw, span, diags) {
                    items.push(Item::Data { kind: DataKind::Dw, bytes, span });
                }
            }
            "dd" => {
                if let Some(bytes) = parse_data_operands(rest, DataKind::Dd, span, diags) {
                    items.push(Item::Data { kind: DataKind::Dd, bytes, span });
                }
            }
            "dq" => {
                if let Some(bytes) = parse_data_operands(rest, DataKind::Dq, span, diags) {
                    items.push(Item::Data { kind: DataKind::Dq, bytes, span });
                }
            }
            _ => {
                // инструкция
                let mut ops = Vec::new();
                if !rest.trim().is_empty() {
                    for part in rest.split(',') {
                        let part = part.trim();
                        match parse_operand(part) {
                            Some(op) => ops.push(op),
                            None => {
                                diags.push(
                                    Diagnostic::error(
                                        "A0002",
                                        span,
                                        format!("не разобрать операнд `{part}`"),
                                    )
                                    .with_hint("поддержаны регистры (rax/eax/...), числа, [mem] и метки"),
                                );
                            }
                        }
                    }
                }
                items.push(Item::Insn { mnem: head_lower, ops, span });
            }
        }
    }

    Program { items }
}

fn parse_data_operands(s: &str, kind: DataKind, span: Span, diags: &mut Diags) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        if chars[i] == '"' {
            if kind != DataKind::Db {
                diags.push(Diagnostic::error("A0010", span, "строковые литералы разрешены только в директиве `db`"));
                return None;
            }
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    match chars[i] {
                        'n' => out.push(b'\n'),
                        'r' => out.push(b'\r'),
                        't' => out.push(b'\t'),
                        '0' => out.push(0),
                        '\\' => out.push(b'\\'),
                        '"' => out.push(b'"'),
                        other => {
                            out.push(b'\\');
                            out.push(other as u8);
                        }
                    }
                } else {
                    let mut buf = [0u8; 4];
                    let enc = chars[i].encode_utf8(&mut buf);
                    out.extend_from_slice(enc.as_bytes());
                }
                i += 1;
            }
            if i < chars.len() && chars[i] == '"' {
                i += 1; // пропускаем закрывающую кавычку
            } else {
                diags.push(Diagnostic::error("A0011", span, "незакрытая строка в директиве данных"));
                return None;
            }
        } else {
            let start = i;
            while i < chars.len() && chars[i] != ',' {
                i += 1;
            }
            let chunk: String = chars[start..i].iter().collect();
            let chunk = chunk.trim();
            if chunk.is_empty() {
                diags.push(Diagnostic::error("A0012", span, "пустое значение в директиве данных"));
                return None;
            }
            let val = match parse_imm(chunk) {
                Some(v) => v,
                None => {
                    diags.push(Diagnostic::error("A0013", span, format!("не удалось разобрать число `{chunk}`")));
                    return None;
                }
            };
            match kind {
                DataKind::Db => {
                    out.push(val as u8);
                }
                DataKind::Dw => {
                    out.extend_from_slice(&(val as i16 as u16).to_le_bytes());
                }
                DataKind::Dd => {
                    out.extend_from_slice(&(val as i32 as u32).to_le_bytes());
                }
                DataKind::Dq => {
                    out.extend_from_slice(&(val as u64).to_le_bytes());
                }
            }
        }

        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i < chars.len() {
            if chars[i] == ',' {
                i += 1;
            } else {
                diags.push(Diagnostic::error("A0014", span, format!("ожидалась запятая перед `{}`", chars[i])));
                return None;
            }
        }
    }
    Some(out)
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars().enumerate().all(|(i, c)| {
            c == '_' || c == '.' || c == '$' || c == '@' || if i == 0 { c.is_alphabetic() || c == '_' || c == '.' } else { c.is_alphanumeric() }
        })
}

fn split_first(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(p) => (&s[..p], s[p..].trim_start()),
        None => (s, ""),
    }
}

fn parse_operand(s: &str) -> Option<Operand> {
    let mut s = s.trim();
    for prefix in &["qword ptr ", "dword ptr ", "byte ptr ", "word ptr ", "offset "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim();
            break;
        }
    }

    if let Some(r) = reg64(s) {
        return Some(Operand::R64(r));
    }
    if let Some(r) = reg32(s) {
        return Some(Operand::R32(r));
    }
    if s.starts_with('[') && s.ends_with(']') {
        let inner = s[1..s.len() - 1].trim();
        // Проверка RIP-relative: [rip + ident], [rel ident] или просто [ident]
        if let Some(rest) = inner.strip_prefix("rip").or_else(|| inner.strip_prefix("RIP")) {
            let rest = rest.trim();
            if let Some(target) = rest.strip_prefix('+') {
                let target = target.trim();
                if is_ident(target) {
                    return Some(Operand::RipRel(target.to_string()));
                }
            }
        }
        if let Some(rest) = inner.strip_prefix("rel ").or_else(|| inner.strip_prefix("REL ")) {
            let target = rest.trim();
            if is_ident(target) {
                return Some(Operand::RipRel(target.to_string()));
            }
        }
        if is_ident(inner) && reg64(inner).is_none() && reg32(inner).is_none() {
            return Some(Operand::RipRel(inner.to_string()));
        }
        return parse_mem(inner).map(Operand::Mem);
    }
    if let Some(imm) = parse_imm(s) {
        return Some(Operand::Imm(imm));
    }
    if is_ident(s) {
        return Some(Operand::Label(s.to_string()));
    }
    None
}

/// Разбор адреса памяти `base [+ index [* scale]] [+/- disp]` (64-битные регистры).
fn parse_mem(inner: &str) -> Option<AsmMemoryOperand> {
    let mut toks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in inner.chars() {
        if c == '+' || c == '-' || c == '*' {
            if !cur.trim().is_empty() {
                toks.push(cur.trim().to_string());
            }
            cur.clear();
            toks.push(c.to_string());
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        toks.push(cur.trim().to_string());
    }
    if toks.is_empty() {
        return None;
    }

    let base = reg64(&toks[0])?;
    let mut index: Option<(AsmRegister64, i32)> = None;
    let mut disp: i64 = 0;

    let mut i = 1;
    while i < toks.len() {
        let sign = match toks[i].as_str() {
            "+" => 1i64,
            "-" => -1i64,
            _ => return None,
        };
        i += 1;
        let t = toks.get(i)?;
        if let Some(r) = reg64(t) {
            let mut scale = 1i32;
            if toks.get(i + 1).map(|s| s == "*").unwrap_or(false) {
                let sc = toks.get(i + 2)?;
                scale = sc.parse::<i32>().ok()?;
                i += 2;
            }
            index = Some((r, scale));
        } else if let Some(n) = parse_imm(t) {
            disp += sign * n;
        } else {
            return None;
        }
        i += 1;
    }

    let mut m: AsmMemoryOperand = base + 0i64;
    if let Some((idx, scale)) = index {
        m = m + idx * scale;
    }
    if disp != 0 {
        m = m + disp;
    }
    Some(m)
}

fn parse_imm(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b.trim()),
        None => (false, s),
    };
    let v = if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        i64::from_str_radix(&h.replace('_', ""), 16).ok()?
    } else if let Some(b) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
        i64::from_str_radix(&b.replace('_', ""), 2).ok()?
    } else {
        body.replace('_', "").parse::<i64>().ok()?
    };
    Some(if neg { -v } else { v })
}

fn reg64(s: &str) -> Option<AsmRegister64> {
    Some(match s.to_lowercase().as_str() {
        "rax" => rax, "rbx" => rbx, "rcx" => rcx, "rdx" => rdx,
        "rsi" => rsi, "rdi" => rdi, "rbp" => rbp, "rsp" => rsp,
        "r8" => r8, "r9" => r9, "r10" => r10, "r11" => r11,
        "r12" => r12, "r13" => r13, "r14" => r14, "r15" => r15,
        _ => return None,
    })
}

fn reg32(s: &str) -> Option<AsmRegister32> {
    Some(match s.to_lowercase().as_str() {
        "eax" => eax, "ebx" => ebx, "ecx" => ecx, "edx" => edx,
        "esi" => esi, "edi" => edi, "ebp" => ebp, "esp" => esp,
        "r8d" => r8d, "r9d" => r9d, "r10d" => r10d, "r11d" => r11d,
        "r12d" => r12d, "r13d" => r13d, "r14d" => r14d, "r15d" => r15d,
        _ => return None,
    })
}

// ---------- кодирование через iced + запись COFF ----------

struct PendingReloc {
    label: CodeLabel,
    offset_in_insn: u64,
    symbol: String,
}

fn encode(prog: &Program, diags: &mut Diags) -> Option<Vec<u8>> {
    let mut a = CodeAssembler::new(64).ok()?;

    let mut cur_sec = ".text";
    let mut data_bytes = Vec::new();
    let mut rdata_bytes = Vec::new();

    let mut text_labels: HashMap<String, CodeLabel> = HashMap::new();
    let mut text_label_order: Vec<String> = Vec::new();
    let mut data_labels: HashMap<String, (&'static str, u64)> = HashMap::new();
    let mut extern_symbols: Vec<String> = Vec::new();
    let mut global_symbols: Vec<String> = Vec::new();

    // 1. Первый проход: секции, метки данных, глобалы и экстерны
    for item in &prog.items {
        match item {
            Item::Section(sec) => {
                let s = sec.trim().to_lowercase();
                if s == ".text" || s == "text" {
                    cur_sec = ".text";
                } else if s == ".data" || s == "data" {
                    cur_sec = ".data";
                } else if s == ".rdata" || s == "rdata" || s == ".rodata" || s == "rodata" {
                    cur_sec = ".rdata";
                } else {
                    diags.push(Diagnostic::warning(
                        "A0007",
                        Span::dummy(),
                        format!("неизвестная секция `{sec}`, трактуется как .data"),
                    ));
                    cur_sec = ".data";
                }
            }
            Item::Global(g) => {
                if !global_symbols.contains(g) {
                    global_symbols.push(g.clone());
                }
            }
            Item::Extern(e) => {
                if !extern_symbols.contains(e) {
                    extern_symbols.push(e.clone());
                }
            }
            Item::Label(name) => {
                match cur_sec {
                    ".text" => {
                        if !text_labels.contains_key(name) {
                            text_labels.insert(name.clone(), a.create_label());
                            text_label_order.push(name.clone());
                        }
                    }
                    ".data" => {
                        data_labels.insert(name.clone(), (".data", data_bytes.len() as u64));
                    }
                    ".rdata" => {
                        data_labels.insert(name.clone(), (".rdata", rdata_bytes.len() as u64));
                    }
                    _ => unreachable!(),
                }
            }
            Item::Data { bytes, .. } => {
                match cur_sec {
                    ".data" => data_bytes.extend_from_slice(bytes),
                    ".rdata" => rdata_bytes.extend_from_slice(bytes),
                    ".text" => {
                        diags.push(Diagnostic::warning(
                            "A0008",
                            Span::dummy(),
                            "данные в .text помещены в .rdata",
                        ));
                        rdata_bytes.extend_from_slice(bytes);
                    }
                    _ => unreachable!(),
                }
            }
            Item::Insn { .. } => {}
        }
    }

    // 2. Второй проход: эмиссия инструкций в .text и регистрация релокаций
    let mut pending_relocs = Vec::new();
    cur_sec = ".text";

    for item in &prog.items {
        match item {
            Item::Section(sec) => {
                let s = sec.trim().to_lowercase();
                if s == ".text" || s == "text" {
                    cur_sec = ".text";
                } else if s == ".data" || s == "data" {
                    cur_sec = ".data";
                } else if s == ".rdata" || s == "rdata" || s == ".rodata" || s == "rodata" {
                    cur_sec = ".rdata";
                } else {
                    cur_sec = ".data";
                }
            }
            Item::Label(name) => {
                if cur_sec == ".text" {
                    if let Some(lbl) = text_labels.get_mut(name) {
                        if let Err(e) = a.set_label(lbl) {
                            diags.push(Diagnostic::error("A0103", Span::dummy(), format!("ошибка метки `{name}`: {e}")));
                        }
                    }
                }
            }
            Item::Insn { mnem, ops, span } => {
                if cur_sec != ".text" {
                    diags.push(Diagnostic::error("A0009", *span, format!("инструкция `{mnem}` вне секции .text")));
                    continue;
                }
                emit_insn(&mut a, mnem, ops, *span, &text_labels, &mut pending_relocs, diags);
            }
            _ => {}
        }
    }

    if diags.has_errors() {
        return None;
    }

    // 3. Кодируем блок и получаем смещения через assemble_options
    let result = match a.assemble_options(0, BlockEncoderOptions::RETURN_NEW_INSTRUCTION_OFFSETS) {
        Ok(r) => r,
        Err(e) => {
            diags.push(Diagnostic::error("A0100", Span::dummy(), format!("ошибка кодирования: {e}")));
            return None;
        }
    };

    let text_label_offsets: Vec<(String, u64)> = text_label_order
        .iter()
        .map(|name| {
            let off = text_labels.get(name).and_then(|lbl| result.label_ip(lbl).ok()).unwrap_or(0);
            (name.clone(), off)
        })
        .collect();

    // Снимаем смещения для релокаций
    struct ResolvedReloc {
        offset: u64,
        symbol: String,
    }
    let mut resolved_relocs = Vec::new();
    for pr in pending_relocs {
        let insn_off = match result.label_ip(&pr.label) {
            Ok(off) => off,
            Err(e) => {
                diags.push(Diagnostic::error("A0104", Span::dummy(), format!("не удалось определить смещение релокации: {e}")));
                continue;
            }
        };
        resolved_relocs.push(ResolvedReloc {
            offset: insn_off + pr.offset_in_insn,
            symbol: pr.symbol,
        });
    }

    let mut code = result.inner.code_buffer;

    // В формате COFF x86-64 поле rel32 содержит неявный адденд linker'а. Обнуляем его перед записью.
    for r in &resolved_relocs {
        let r_idx = r.offset as usize;
        if r_idx + 4 <= code.len() {
            code[r_idx..r_idx + 4].copy_from_slice(&[0, 0, 0, 0]);
        }
    }

    // 4. Собираем COFF-объектник
    let mut obj = Object::new(BinaryFormat::Coff, Architecture::X86_64, Endianness::Little);
    let s_text = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
    let text_base = obj.append_section_data(s_text, &code, 16);

    let (s_data, data_base) = if !data_bytes.is_empty() || data_labels.values().any(|(s, _)| *s == ".data") {
        let s = obj.add_section(Vec::new(), b".data".to_vec(), SectionKind::Data);
        let b = obj.append_section_data(s, &data_bytes, 16);
        (Some(s), b)
    } else {
        (None, 0)
    };

    let (s_rdata, rdata_base) = if !rdata_bytes.is_empty() || data_labels.values().any(|(s, _)| *s == ".rdata") {
        let s = obj.add_section(Vec::new(), b".rdata".to_vec(), SectionKind::ReadOnlyData);
        let b = obj.append_section_data(s, &rdata_bytes, 16);
        (Some(s), b)
    } else {
        (None, 0)
    };

    let mut sym_map: HashMap<String, object::write::SymbolId> = HashMap::new();

    // Метки в .text
    for (name, off) in &text_label_offsets {
        let is_global = global_symbols.contains(name);
        let sym_id = obj.add_symbol(Symbol {
            name: name.clone().into_bytes(),
            value: text_base + off,
            size: 0,
            kind: SymbolKind::Text,
            scope: if is_global { SymbolScope::Linkage } else { SymbolScope::Compilation },
            weak: false,
            section: SymbolSection::Section(s_text),
            flags: SymbolFlags::None,
        });
        sym_map.insert(name.clone(), sym_id);
    }

    // Метки в .data и .rdata
    for (name, (sec_name, off)) in &data_labels {
        let is_global = global_symbols.contains(name);
        let (sec_id, base) = if *sec_name == ".data" {
            (s_data.unwrap(), data_base)
        } else {
            (s_rdata.unwrap(), rdata_base)
        };
        let sym_id = obj.add_symbol(Symbol {
            name: name.clone().into_bytes(),
            value: base + off,
            size: 0,
            kind: SymbolKind::Data,
            scope: if is_global { SymbolScope::Linkage } else { SymbolScope::Compilation },
            weak: false,
            section: SymbolSection::Section(sec_id),
            flags: SymbolFlags::None,
        });
        sym_map.insert(name.clone(), sym_id);
    }

    // Внешние символы (extern)
    for name in &extern_symbols {
        if !sym_map.contains_key(name) {
            let sym_id = obj.add_symbol(Symbol {
                name: name.clone().into_bytes(),
                value: 0,
                size: 0,
                kind: SymbolKind::Text,
                scope: SymbolScope::Linkage,
                weak: false,
                section: SymbolSection::Undefined,
                flags: SymbolFlags::None,
            });
            sym_map.insert(name.clone(), sym_id);
        }
    }

    // Релокации секции .text
    for r in resolved_relocs {
        let sym_id = match sym_map.get(&r.symbol) {
            Some(id) => *id,
            None => {
                let id = obj.add_symbol(Symbol {
                    name: r.symbol.clone().into_bytes(),
                    value: 0,
                    size: 0,
                    kind: SymbolKind::Text,
                    scope: SymbolScope::Linkage,
                    weak: false,
                    section: SymbolSection::Undefined,
                    flags: SymbolFlags::None,
                });
                sym_map.insert(r.symbol.clone(), id);
                id
            }
        };

        if let Err(e) = obj.add_relocation(s_text, object::write::Relocation {
            offset: r.offset,
            symbol: sym_id,
            addend: -4,
            flags: RelocationFlags::Generic {
                kind: RelocationKind::Relative,
                encoding: RelocationEncoding::Generic,
                size: 32,
            },
        }) {
            diags.push(Diagnostic::error("A0105", Span::dummy(), format!("ошибка добавления релокации: {e}")));
        }
    }

    match obj.write() {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            diags.push(Diagnostic::error("A0101", Span::dummy(), format!("не удалось записать объектник: {e}")));
            None
        }
    }
}

fn is_branch_mnem(mnem: &str) -> bool {
    matches!(
        mnem,
        "jmp"
            | "je"
            | "jne"
            | "jz"
            | "jnz"
            | "ja"
            | "jae"
            | "jb"
            | "jbe"
            | "jg"
            | "jge"
            | "jl"
            | "jle"
            | "js"
            | "jns"
            | "call"
            | "loop"
    )
}

fn emit_branch(
    a: &mut CodeAssembler,
    mnem: &str,
    target: &str,
    span: Span,
    labels: &HashMap<String, CodeLabel>,
    pending_relocs: &mut Vec<PendingReloc>,
    diags: &mut Diags,
) -> Result<(), iced_x86::IcedError> {
    if let Some(lbl) = labels.get(target) {
        match mnem {
            "jmp" => a.jmp(*lbl),
            "call" => a.call(*lbl),
            "je" | "jz" => a.je(*lbl),
            "jne" | "jnz" => a.jne(*lbl),
            "jl" => a.jl(*lbl),
            "jle" => a.jle(*lbl),
            "jg" => a.jg(*lbl),
            "jge" => a.jge(*lbl),
            "ja" => a.ja(*lbl),
            "jae" => a.jae(*lbl),
            "jb" => a.jb(*lbl),
            "jbe" => a.jbe(*lbl),
            "js" => a.js(*lbl),
            "jns" => a.jns(*lbl),
            "loop" => a.loop_(*lbl),
            _ => unreachable!(),
        }
    } else if matches!(mnem, "call" | "jmp") {
        // Внешний переход / вызов: создаём метку на инструкции и планируем COFF-релокацию
        let mut pr_lbl = a.create_label();
        a.set_label(&mut pr_lbl)?;
        let res = if mnem == "call" { a.call(0u64) } else { a.jmp(0u64) };
        res?;
        pending_relocs.push(PendingReloc {
            label: pr_lbl,
            offset_in_insn: 1, // смещение rel32 в call/jmp (после опкода 0xE8 / 0xE9)
            symbol: target.to_string(),
        });
        Ok(())
    } else {
        diags.push(
            Diagnostic::error("A0006", span, format!("неизвестная метка перехода `{target}`"))
                .with_hint(format!("объявите метку `{target}:` внутри файла")),
        );
        Ok(())
    }
}

/// Кодирует одну инструкцию, добавляя её в ассемблер `a`.
fn emit_insn(
    a: &mut CodeAssembler,
    mnem: &str,
    ops: &[Operand],
    span: Span,
    labels: &HashMap<String, CodeLabel>,
    pending_relocs: &mut Vec<PendingReloc>,
    diags: &mut Diags,
) {
    use Operand::*;

    // Ветвления и вызовы
    if is_branch_mnem(mnem) {
        if let [Label(target)] = ops {
            if let Err(e) = emit_branch(a, mnem, target, span, labels, pending_relocs, diags) {
                diags.push(Diagnostic::error("A0102", span, format!("не закодировать `{mnem}`: {e}")));
            }
            return;
        } else if matches!(mnem, "jmp" | "call") {
            if let [R64(r)] = ops {
                let res = if mnem == "jmp" { a.jmp(*r) } else { a.call(*r) };
                if let Err(e) = res {
                    diags.push(Diagnostic::error("A0102", span, format!("не закодировать `{mnem}`: {e}")));
                }
                return;
            }
        }
    }

    // RIP-relative операнды (lea reg, [rip + sym], mov reg, [rip + sym], mov [rip + sym], reg)
    if let ("lea", [R64(d), RipRel(sym)]) = (mnem, ops) {
        let mut pr_lbl = a.create_label();
        if let Err(e) = a.set_label(&mut pr_lbl) {
            diags.push(Diagnostic::error("A0103", span, format!("ошибка метки: {e}")));
            return;
        }
        if let Err(e) = a.lea(*d, qword_ptr(pr_lbl)) {
            diags.push(Diagnostic::error("A0102", span, format!("не закодировать `lea`: {e}")));
            return;
        }
        pending_relocs.push(PendingReloc {
            label: pr_lbl,
            offset_in_insn: 3, // смещение rel32 в `48 8d 05 [rel32]`
            symbol: sym.clone(),
        });
        return;
    }

    if let ("mov", [R64(d), RipRel(sym)]) = (mnem, ops) {
        let mut pr_lbl = a.create_label();
        if let Err(e) = a.set_label(&mut pr_lbl) {
            diags.push(Diagnostic::error("A0103", span, format!("ошибка метки: {e}")));
            return;
        }
        if let Err(e) = a.mov(*d, qword_ptr(pr_lbl)) {
            diags.push(Diagnostic::error("A0102", span, format!("не закодировать `mov`: {e}")));
            return;
        }
        pending_relocs.push(PendingReloc {
            label: pr_lbl,
            offset_in_insn: 3, // смещение rel32 в `48 8b 05 [rel32]`
            symbol: sym.clone(),
        });
        return;
    }

    if let ("mov", [RipRel(sym), R64(s)]) = (mnem, ops) {
        let mut pr_lbl = a.create_label();
        if let Err(e) = a.set_label(&mut pr_lbl) {
            diags.push(Diagnostic::error("A0103", span, format!("ошибка метки: {e}")));
            return;
        }
        if let Err(e) = a.mov(qword_ptr(pr_lbl), *s) {
            diags.push(Diagnostic::error("A0102", span, format!("не закодировать `mov`: {e}")));
            return;
        }
        pending_relocs.push(PendingReloc {
            label: pr_lbl,
            offset_in_insn: 3, // смещение rel32 в `48 89 05 [rel32]`
            symbol: sym.clone(),
        });
        return;
    }

    if let ("mov", [R32(d), RipRel(sym)]) = (mnem, ops) {
        let mut pr_lbl = a.create_label();
        if let Err(e) = a.set_label(&mut pr_lbl) {
            diags.push(Diagnostic::error("A0103", span, format!("ошибка метки: {e}")));
            return;
        }
        if let Err(e) = a.mov(*d, dword_ptr(pr_lbl)) {
            diags.push(Diagnostic::error("A0102", span, format!("не закодировать `mov`: {e}")));
            return;
        }
        pending_relocs.push(PendingReloc {
            label: pr_lbl,
            offset_in_insn: 2, // смещение rel32 в `8b 05 [rel32]`
            symbol: sym.clone(),
        });
        return;
    }

    if let ("mov", [RipRel(sym), R32(s)]) = (mnem, ops) {
        let mut pr_lbl = a.create_label();
        if let Err(e) = a.set_label(&mut pr_lbl) {
            diags.push(Diagnostic::error("A0103", span, format!("ошибка метки: {e}")));
            return;
        }
        if let Err(e) = a.mov(dword_ptr(pr_lbl), *s) {
            diags.push(Diagnostic::error("A0102", span, format!("не закодировать `mov`: {e}")));
            return;
        }
        pending_relocs.push(PendingReloc {
            label: pr_lbl,
            offset_in_insn: 2, // смещение rel32 в `89 05 [rel32]`
            symbol: sym.clone(),
        });
        return;
    }

    // Семейство двухоперандных ALU с одинаковыми формами — отдельно.
    if matches!(mnem, "add" | "sub" | "and" | "or" | "xor" | "cmp") {
        alu2(a, mnem, ops, span, diags);
        return;
    }

    let res: Result<(), iced_x86::IcedError> = match (mnem, ops) {
        // 0-операндные
        ("ret", []) => a.ret(),
        ("nop", []) => a.nop(),
        ("syscall", []) => a.syscall(),
        ("leave", []) => a.leave(),
        ("cqo", []) => a.cqo(),

        // 1-операндные
        ("push", [R64(r)]) => a.push(*r),
        ("pop", [R64(r)]) => a.pop(*r),
        ("inc", [R64(r)]) => a.inc(*r),
        ("dec", [R64(r)]) => a.dec(*r),
        ("neg", [R64(r)]) => a.neg(*r),
        ("not", [R64(r)]) => a.not(*r),
        ("inc", [R32(r)]) => a.inc(*r),
        ("dec", [R32(r)]) => a.dec(*r),

        // 2-операндные: mov
        ("mov", [R64(d), R64(s)]) => a.mov(*d, *s),
        ("mov", [R64(d), Imm(i)]) => a.mov(*d, *i),
        ("mov", [R32(d), R32(s)]) => a.mov(*d, *s),
        ("mov", [R32(d), Imm(i)]) => a.mov(*d, *i as i32),
        // mov с памятью
        ("mov", [R64(d), Mem(m)]) => a.mov(*d, qword_ptr(*m)),
        ("mov", [Mem(m), R64(s)]) => a.mov(qword_ptr(*m), *s),
        ("mov", [Mem(m), Imm(i)]) => a.mov(qword_ptr(*m), *i as i32),
        ("lea", [R64(d), Mem(m)]) => a.lea(*d, qword_ptr(*m)),

        ("imul", [R64(d), R64(s)]) => a.imul_2(*d, *s),
        ("imul", [R64(d), Mem(m)]) => a.imul_2(*d, qword_ptr(*m)),

        _ => {
            diags.push(
                Diagnostic::error("A0004", span, format!("не поддержанная инструкция или форма: `{mnem}`"))
                    .with_hint("поддержаны: mov/add/sub/and/or/xor/cmp/push/pop/inc/dec/neg/not/imul/ret/nop/syscall, а также jmp/jcc/call/loop"),
            );
            return;
        }
    };
    if let Err(e) = res {
        diags.push(Diagnostic::error("A0102", span, format!("не закодировать `{mnem}`: {e}")));
    }
}

/// Двухоперандные ALU-инструкции с одинаковыми формами операндов.
fn alu2(a: &mut CodeAssembler, mnem: &str, ops: &[Operand], span: Span, diags: &mut Diags) {
    use Operand::*;
    let res: Result<(), iced_x86::IcedError> = match (mnem, ops) {
        ("add", [R64(d), R64(s)]) => a.add(*d, *s),
        ("add", [R64(d), Imm(i)]) => a.add(*d, *i as i32),
        ("add", [R32(d), R32(s)]) => a.add(*d, *s),
        ("add", [R32(d), Imm(i)]) => a.add(*d, *i as i32),
        ("sub", [R64(d), R64(s)]) => a.sub(*d, *s),
        ("sub", [R64(d), Imm(i)]) => a.sub(*d, *i as i32),
        ("sub", [R32(d), R32(s)]) => a.sub(*d, *s),
        ("sub", [R32(d), Imm(i)]) => a.sub(*d, *i as i32),
        ("and", [R64(d), R64(s)]) => a.and(*d, *s),
        ("and", [R64(d), Imm(i)]) => a.and(*d, *i as i32),
        ("or", [R64(d), R64(s)]) => a.or(*d, *s),
        ("or", [R64(d), Imm(i)]) => a.or(*d, *i as i32),
        ("xor", [R64(d), R64(s)]) => a.xor(*d, *s),
        ("xor", [R64(d), Imm(i)]) => a.xor(*d, *i as i32),
        ("xor", [R32(d), R32(s)]) => a.xor(*d, *s),
        ("cmp", [R64(d), R64(s)]) => a.cmp(*d, *s),
        ("cmp", [R64(d), Imm(i)]) => a.cmp(*d, *i as i32),
        ("cmp", [R32(d), R32(s)]) => a.cmp(*d, *s),
        ("cmp", [R32(d), Imm(i)]) => a.cmp(*d, *i as i32),
        // ALU с памятью (reg64, [mem]) и ([mem], reg64)
        ("add", [R64(d), Mem(m)]) => a.add(*d, qword_ptr(*m)),
        ("add", [Mem(m), R64(s)]) => a.add(qword_ptr(*m), *s),
        ("sub", [R64(d), Mem(m)]) => a.sub(*d, qword_ptr(*m)),
        ("sub", [Mem(m), R64(s)]) => a.sub(qword_ptr(*m), *s),
        ("and", [R64(d), Mem(m)]) => a.and(*d, qword_ptr(*m)),
        ("or", [R64(d), Mem(m)]) => a.or(*d, qword_ptr(*m)),
        ("xor", [R64(d), Mem(m)]) => a.xor(*d, qword_ptr(*m)),
        ("cmp", [R64(d), Mem(m)]) => a.cmp(*d, qword_ptr(*m)),
        ("cmp", [Mem(m), R64(s)]) => a.cmp(qword_ptr(*m), *s),
        _ => {
            diags.push(
                Diagnostic::error("A0005", span, format!("не поддержанная форма `{mnem}`"))
                    .with_hint("формы: (reg, reg) и (reg, imm) для r64/r32"),
            );
            return;
        }
    };
    if let Err(e) = res {
        diags.push(Diagnostic::error("A0102", span, format!("не закодировать `{mnem}`: {e}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assemble_basic() {
        let src = r#"
            section .text
            global goraw_add
            goraw_add:
                mov rax, rcx
                add rax, rdx
                ret
        "#;
        let (obj, diags) = assemble("test.asm", src);
        assert!(!diags.has_errors(), "diags: {:?}", diags.render_human());
        let bytes = obj.expect("obj bytes expected");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_assemble_data_and_extern() {
        let src = r#"
            section .rdata
            msg: db "Hello, gorawas!", 10, 0

            section .text
            extern printf
            global main
            main:
                sub rsp, 40
                lea rcx, [rip + msg]
                call printf
                add rsp, 40
                xor eax, eax
                ret
        "#;
        let (obj, diags) = assemble("hello.asm", src);
        assert!(!diags.has_errors(), "diags: {:?}", diags.render_human());
        let bytes = obj.expect("obj bytes expected");
        assert!(!bytes.is_empty());

        let parsed = object::read::File::parse(&*bytes).expect("parse COFF");
        use object::{Object as _, ObjectSection as _, ObjectSymbol as _};

        let sym_names: Vec<String> = parsed.symbols().filter_map(|s| s.name().ok().map(String::from)).collect();
        assert!(sym_names.contains(&"main".to_string()));
        assert!(sym_names.contains(&"msg".to_string()));
        assert!(sym_names.contains(&"printf".to_string()));

        let text_sec = parsed.section_by_name(".text").expect(".text section");
        let relocs: Vec<_> = text_sec.relocations().collect();
        assert_eq!(relocs.len(), 2, "expected 2 relocations (lea msg + call printf)");
    }
}
