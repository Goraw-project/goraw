//! `gorawas` — нативный ассемблер Goraw-asm (собственный Intel-диалект).
//!
//! Clean-room: мы не копируем NASM/MASM. «Наше» — фронтенд (лексер/парсер
//! диалекта, резолв символов) и запись объектника. Кодирование инструкций
//! (ModRM/REX/VEX/EVEX) делегируется проверенному `iced-x86`, формат COFF —
//! крейту `object`. Ошибки идут через общий `crate::diag`, поэтому у
//! ассемблера — та же LLM-JSON диагностика, что и у компилятора.
//!
//! Это вертикальный срез: регистровые/непосредственные операнды, метки как
//! символы, базовый набор инструкций. Память, релокации внешних символов,
//! ветвления, данные (`db/dq`) — следующие шаги (см. docs/ROADMAP.md).

use crate::diag::{Diagnostic, Diags, Pos, Span};
use iced_x86::code_asm::*;
use iced_x86::{BlockEncoder, BlockEncoderOptions, InstructionBlock};

use object::write::{Object, Symbol, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, SectionKind, SymbolFlags, SymbolKind, SymbolScope,
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
    Section(String),
    Insn { mnem: String, ops: Vec<Operand>, span: Span },
}

#[derive(Debug, Clone, Copy)]
enum Operand {
    R64(AsmRegister64),
    R32(AsmRegister32),
    Imm(i64),
    Mem(AsmMemoryOperand),
}

struct Program {
    items: Vec<Item>,
    globals: Vec<String>,
}

/// Разбор построчно: `;` — комментарий, `name:` — метка, `section/global` —
/// директивы, иначе инструкция `mnem op, op`.
fn parse(src: &str, diags: &mut Diags) -> Program {
    let mut items = Vec::new();
    let mut globals = Vec::new();

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

        // метка: одиночный идентификатор с ':'
        if let Some(name) = line.strip_suffix(':') {
            let name = name.trim();
            if is_ident(name) {
                items.push(Item::Label(name.to_string()));
                continue;
            } else {
                diags.push(Diagnostic::error("A0001", span, format!("некорректная метка `{name}`")));
                continue;
            }
        }

        // директива или инструкция
        let (head, rest) = split_first(line);
        match head.to_lowercase().as_str() {
            "section" => items.push(Item::Section(rest.trim().to_string())),
            "global" | "globl" => {
                let name = rest.trim().to_string();
                globals.push(name.clone());
                items.push(Item::Global(name));
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
                                    .with_hint("поддержаны регистры (rax/eax/...) и числа"),
                                );
                            }
                        }
                    }
                }
                items.push(Item::Insn { mnem: head.to_lowercase(), ops, span });
            }
        }
    }

    Program { items, globals }
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars().enumerate().all(|(i, c)| {
            c == '_' || c == '.' || c == '$' || if i == 0 { c.is_alphabetic() || c == '_' || c == '.' } else { c.is_alphanumeric() }
        })
}

fn split_first(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(p) => (&s[..p], s[p..].trim_start()),
        None => (s, ""),
    }
}

fn parse_operand(s: &str) -> Option<Operand> {
    if let Some(r) = reg64(s) {
        return Some(Operand::R64(r));
    }
    if let Some(r) = reg32(s) {
        return Some(Operand::R32(r));
    }
    if s.starts_with('[') && s.ends_with(']') {
        return parse_mem(&s[1..s.len() - 1]).map(Operand::Mem);
    }
    parse_imm(s).map(Operand::Imm)
}

/// Разбор адреса памяти `base [+ index [* scale]] [+/- disp]` (64-битные
/// регистры). Возвращает AsmMemoryOperand без размера — размер задаётся при
/// эмиссии по парному регистру.
fn parse_mem(inner: &str) -> Option<AsmMemoryOperand> {
    // Токенизация: идентификаторы, числа, операторы + - *
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
            // индексный регистр, возможно со шкалой
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

fn encode(prog: &Program, diags: &mut Diags) -> Option<Vec<u8>> {
    let mut a = CodeAssembler::new(64).ok()?;

    // Метка -> индекс инструкции, перед которой она стоит.
    let mut label_at: Vec<(String, usize)> = Vec::new();

    for item in &prog.items {
        match item {
            Item::Label(name) => {
                let idx = a.instructions().len();
                label_at.push((name.clone(), idx));
            }
            Item::Global(_) | Item::Section(_) => {}
            Item::Insn { mnem, ops, span } => {
                emit_insn(&mut a, mnem, ops, *span, diags);
            }
        }
    }
    if diags.has_errors() {
        return None;
    }

    // Кодируем блок и получаем смещение каждой инструкции.
    let instrs = a.instructions().to_vec();
    let block = InstructionBlock::new(&instrs, 0);
    let result = match BlockEncoder::encode(64, block, BlockEncoderOptions::RETURN_NEW_INSTRUCTION_OFFSETS) {
        Ok(r) => r,
        Err(e) => {
            diags.push(Diagnostic::error("A0100", Span::dummy(), format!("ошибка кодирования: {e}")));
            return None;
        }
    };
    let code = result.code_buffer;
    let offsets = result.new_instruction_offsets;

    let label_off = |idx: usize| -> u64 {
        if idx < offsets.len() {
            offsets[idx] as u64
        } else {
            code.len() as u64
        }
    };

    // Собираем COFF-объектник.
    let mut obj = Object::new(BinaryFormat::Coff, Architecture::X86_64, Endianness::Little);
    let text = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
    let base = obj.append_section_data(text, &code, 16);

    for (name, idx) in &label_at {
        let is_global = prog.globals.iter().any(|g| g == name);
        let value = base + label_off(*idx);
        obj.add_symbol(Symbol {
            name: name.clone().into_bytes(),
            value,
            size: 0,
            kind: SymbolKind::Text,
            scope: if is_global { SymbolScope::Linkage } else { SymbolScope::Compilation },
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
    }

    match obj.write() {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            diags.push(Diagnostic::error("A0101", Span::dummy(), format!("не удалось записать объектник: {e}")));
            None
        }
    }
}

/// Кодирует одну инструкцию, добавляя её в ассемблер `a`.
fn emit_insn(a: &mut CodeAssembler, mnem: &str, ops: &[Operand], span: Span, diags: &mut Diags) {
    use Operand::*;

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
        // mov с памятью (размер qword по r64)
        ("mov", [R64(d), Mem(m)]) => a.mov(*d, qword_ptr(*m)),
        ("mov", [Mem(m), R64(s)]) => a.mov(qword_ptr(*m), *s),
        ("mov", [Mem(m), Imm(i)]) => a.mov(qword_ptr(*m), *i as i32),
        ("lea", [R64(d), Mem(m)]) => a.lea(*d, qword_ptr(*m)),

        ("imul", [R64(d), R64(s)]) => a.imul_2(*d, *s),
        ("imul", [R64(d), Mem(m)]) => a.imul_2(*d, qword_ptr(*m)),

        _ => {
            diags.push(
                Diagnostic::error("A0004", span, format!("не поддержанная инструкция или форма: `{mnem}`"))
                    .with_hint("в этом срезе: mov/add/sub/and/or/xor/cmp/push/pop/inc/dec/neg/not/imul/ret/nop/syscall"),
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
