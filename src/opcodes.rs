//! Модуль извлечения и форматирования опкодов (машинного кода x86-64) из скомпилированного кода Goraw.
//!
//! Позволяет получать:
//! - Массив опкодов в синтаксисе Goraw (`const FOO_OPCODES: [u8; N] = [ ... ];`)
//! - Массив опкодов в синтаксисе C/C++ (`const unsigned char foo_opcodes[] = { ... };`)
//! - Массив опкодов в синтаксисе Rust (`pub const FOO_OPCODES: &[u8] = &[ ... ];`)
//! - Строку шеллкода в hex-формате (`\x48\x89\x5c...`)
//! - Дизассемблерный листинг с опкодами рядом с инструкциями (через `iced-x86`)
//! - Чистый бинарный файл опкодов (`.bin`)

use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use object::{Object, ObjectSection, ObjectSymbol, SectionKind, SymbolKind, SymbolSection};
use std::path::Path;
use std::process::Command;

/// Формат вывода опкодов
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpcodeFormat {
    Goraw,
    C,
    Rust,
    Hex,
    Asm,
    Bin,
}

impl OpcodeFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "goraw" | "gw" => Some(OpcodeFormat::Goraw),
            "c" | "cpp" | "c++" => Some(OpcodeFormat::C),
            "rust" | "rs" => Some(OpcodeFormat::Rust),
            "hex" | "shellcode" => Some(OpcodeFormat::Hex),
            "asm" | "disasm" => Some(OpcodeFormat::Asm),
            "bin" | "raw" => Some(OpcodeFormat::Bin),
            _ => None,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            OpcodeFormat::Goraw => "Массив опкодов Goraw (`const NAME: [u8; N] = [0x...];`)",
            OpcodeFormat::C => "Массив опкодов C/C++ (`const unsigned char name[] = { 0x... };`)",
            OpcodeFormat::Rust => "Массив опкодов Rust (`pub const NAME: &[u8] = &[0x...];`)",
            OpcodeFormat::Hex => "Шелл-код / Hex-строка (`\\x48\\x89...`)",
            OpcodeFormat::Asm => "Дизассемблер с опкодами инструкций",
            OpcodeFormat::Bin => "Сырой бинарный дамп опкодов (.bin)",
        }
    }
}

/// Разобранная инструкция
#[derive(Debug, Clone)]
pub struct DisassembledInsn {
    pub ip: u64,
    pub bytes: Vec<u8>,
    pub text: String,
}

/// Опкоды отдельной функции
#[derive(Debug, Clone)]
pub struct FunctionOpcodes {
    pub name: String,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub instructions: Vec<DisassembledInsn>,
}

/// Полный отчет по опкодам модуля
#[derive(Debug, Clone)]
pub struct OpcodesReport {
    pub functions: Vec<FunctionOpcodes>,
    pub all_text: Vec<u8>,
}

impl OpcodesReport {
    /// Отрендерить отчет в выбранном формате
    pub fn render(&self, format: OpcodeFormat, filter_func: Option<&str>) -> Result<Vec<u8>, String> {
        if format == OpcodeFormat::Bin {
            if let Some(target) = filter_func {
                if let Some(f) = self.functions.iter().find(|f| f.name == target) {
                    return Ok(f.bytes.clone());
                } else {
                    return Err(format!("функция `{target}` не найдена среди символов"));
                }
            } else {
                return Ok(self.all_text.clone());
            }
        }

        let mut out = String::new();
        let target_funcs: Vec<&FunctionOpcodes> = if let Some(target) = filter_func {
            let matches: Vec<&FunctionOpcodes> = self.functions.iter().filter(|f| f.name == target).collect();
            if matches.is_empty() {
                return Err(format!("функция `{target}` не найдена среди символов"));
            }
            matches
        } else {
            self.functions.iter().collect()
        };

        match format {
            OpcodeFormat::Goraw => {
                out.push_str("// Опкоды x86-64, сгенерированные компилятором Goraw\n\n");
                for f in &target_funcs {
                    let sanitized = sanitize_ident(&f.name);
                    let upper = sanitized.to_uppercase();
                    out.push_str(&format!(
                        "// Функция `{}` (размер: {} байт)\n",
                        f.name,
                        f.bytes.len()
                    ));
                    out.push_str(&format!(
                        "const {upper}_OPCODES: [u8; {}] = [\n",
                        f.bytes.len()
                    ));
                    format_byte_array(&f.bytes, &mut out, "    ");
                    out.push_str("];\n\n");
                }
                if filter_func.is_none() && !self.all_text.is_empty() {
                    out.push_str(&format!(
                        "// Полная секция .text (размер: {} байт)\n",
                        self.all_text.len()
                    ));
                    out.push_str(&format!(
                        "const TEXT_SECTION_OPCODES: [u8; {}] = [\n",
                        self.all_text.len()
                    ));
                    format_byte_array(&self.all_text, &mut out, "    ");
                    out.push_str("];\n");
                }
            }
            OpcodeFormat::C => {
                out.push_str("// x86-64 opcodes generated by Goraw compiler\n");
                out.push_str("#pragma once\n\n");
                for f in &target_funcs {
                    let sanitized = sanitize_ident(&f.name);
                    out.push_str(&format!(
                        "// Function `{}` (size: {} bytes)\n",
                        f.name,
                        f.bytes.len()
                    ));
                    out.push_str(&format!(
                        "const unsigned char {}_opcodes[{}] = {{\n",
                        sanitized,
                        f.bytes.len()
                    ));
                    format_byte_array(&f.bytes, &mut out, "    ");
                    out.push_str("};\n\n");
                }
                if filter_func.is_none() && !self.all_text.is_empty() {
                    out.push_str(&format!(
                        "// Entire .text section (size: {} bytes)\n",
                        self.all_text.len()
                    ));
                    out.push_str(&format!(
                        "const unsigned char text_section_opcodes[{}] = {{\n",
                        self.all_text.len()
                    ));
                    format_byte_array(&self.all_text, &mut out, "    ");
                    out.push_str("};\n");
                }
            }
            OpcodeFormat::Rust => {
                out.push_str("// x86-64 opcodes generated by Goraw compiler\n\n");
                for f in &target_funcs {
                    let sanitized = sanitize_ident(&f.name);
                    let upper = sanitized.to_uppercase();
                    out.push_str(&format!(
                        "/// Функция `{}` (размер: {} байт)\n",
                        f.name,
                        f.bytes.len()
                    ));
                    out.push_str(&format!(
                        "pub const {upper}_OPCODES: &[u8] = &[\n"
                    ));
                    format_byte_array(&f.bytes, &mut out, "    ");
                    out.push_str("];\n\n");
                }
                if filter_func.is_none() && !self.all_text.is_empty() {
                    out.push_str(&format!(
                        "/// Полная секция .text (размер: {} байт)\n",
                        self.all_text.len()
                    ));
                    out.push_str(&format!(
                        "pub const TEXT_SECTION_OPCODES: &[u8] = &[\n"
                    ));
                    format_byte_array(&self.all_text, &mut out, "    ");
                    out.push_str("];\n");
                }
            }
            OpcodeFormat::Hex => {
                for f in &target_funcs {
                    out.push_str(&format!("// {} ({} bytes):\n\"", f.name, f.bytes.len()));
                    for b in &f.bytes {
                        out.push_str(&format!("\\x{b:02x}"));
                    }
                    out.push_str("\"\n\n");
                }
            }
            OpcodeFormat::Asm => {
                for f in &target_funcs {
                    out.push_str(&format!("; ===== {} ({} bytes) =====\n", f.name, f.bytes.len()));
                    out.push_str(&format!("{}:\n", f.name));
                    for insn in &f.instructions {
                        let hex_bytes: Vec<String> = insn.bytes.iter().map(|b| format!("{b:02x}")).collect();
                        let hex_str = hex_bytes.join(" ");
                        out.push_str(&format!("  {:08x}:  {:<24}  {}\n", insn.ip, hex_str, insn.text));
                    }
                    out.push('\n');
                }
            }
            OpcodeFormat::Bin => unreachable!(),
        }

        Ok(out.into_bytes())
    }
}

fn sanitize_ident(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    if s.is_empty() {
        "func".to_string()
    } else {
        s
    }
}

fn format_byte_array(bytes: &[u8], out: &mut String, indent: &str) {
    for chunk in bytes.chunks(16) {
        out.push_str(indent);
        for b in chunk {
            out.push_str(&format!("0x{b:02x}, "));
        }
        out.push('\n');
    }
}

/// Извлечь опкоды из скомпилированного объектного файла (.obj / .o)
pub fn extract_opcodes_from_obj(obj_bytes: &[u8]) -> Result<OpcodesReport, String> {
    let file = object::File::parse(obj_bytes)
        .map_err(|e| format!("ошибка разбора объектного файла: {e}"))?;

    let mut all_text = Vec::new();
    let mut functions = Vec::new();

    // Собираем все секции с кодом (.text, .text$*)
    for section in file.sections() {
        let is_text = section.kind() == SectionKind::Text
            || section
                .name()
                .map(|n| n.starts_with(".text"))
                .unwrap_or(false);

        if is_text {
            if let Ok(data) = section.data() {
                if !data.is_empty() {
                    all_text.extend_from_slice(data);
                }
            }
        }
    }

    // Собираем символы функций
    let mut symbols_list: Vec<(String, u64, u64, Vec<u8>)> = Vec::new();

    for sym in file.symbols() {
        if sym.kind() == SymbolKind::Text {
            let name = match sym.name() {
                Ok(n) => n.to_string(),
                Err(_) => continue,
            };

            // Пропускаем служебные / пустые символы
            if name.is_empty() || name.starts_with('.') || name.starts_with('$') {
                continue;
            }

            let sec_idx = match sym.section() {
                SymbolSection::Section(idx) => idx,
                _ => continue,
            };

            if let Ok(sec) = file.section_by_index(sec_idx) {
                if let Ok(sec_data) = sec.data() {
                    let addr = sym.address();
                    let size = sym.size();
                    if addr as usize <= sec_data.len() {
                        let fn_data = if size > 0 && (addr + size) as usize <= sec_data.len() {
                            sec_data[addr as usize..(addr + size) as usize].to_vec()
                        } else {
                            sec_data[addr as usize..].to_vec()
                        };
                        symbols_list.push((name, addr, size, fn_data));
                    }
                }
            }
        }
    }

    // Если размер символа в COFF был 0, вычисляем размер как разницу до следующего символа в секции
    symbols_list.sort_by_key(|s| s.1);
    let n = symbols_list.len();
    for i in 0..n {
        let (name, addr, orig_size, mut data) = symbols_list[i].clone();
        if orig_size == 0 && i + 1 < n {
            let next_addr = symbols_list[i + 1].1;
            if next_addr > addr && (next_addr - addr) as usize <= data.len() {
                data.truncate((next_addr - addr) as usize);
            }
        }

        // Дизассемблируем через iced-x86
        let instructions = disassemble_bytes(&data, addr);
        functions.push(FunctionOpcodes {
            name,
            offset: addr,
            bytes: data,
            instructions,
        });
    }

    // Если символы были отстрипаны, но есть .text, создаем обобщенную функцию `text`
    if functions.is_empty() && !all_text.is_empty() {
        let insns = disassemble_bytes(&all_text, 0);
        functions.push(FunctionOpcodes {
            name: "text_section".to_string(),
            offset: 0,
            bytes: all_text.clone(),
            instructions: insns,
        });
    }

    Ok(OpcodesReport {
        functions,
        all_text,
    })
}

/// Дизассемблирование среза байтов x86-64
pub fn disassemble_bytes(bytes: &[u8], base_ip: u64) -> Vec<DisassembledInsn> {
    if bytes.is_empty() {
        return Vec::new();
    }

    let mut decoder = Decoder::with_ip(64, bytes, base_ip, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut output = String::new();
    let mut insn = Instruction::default();
    let mut result = Vec::new();

    while decoder.can_decode() {
        decoder.decode_out(&mut insn);
        output.clear();
        formatter.format(&insn, &mut output);

        let ip_offset = (insn.ip() - base_ip) as usize;
        let len = insn.len();
        let insn_bytes = if ip_offset + len <= bytes.len() {
            bytes[ip_offset..ip_offset + len].to_vec()
        } else {
            Vec::new()
        };

        result.push(DisassembledInsn {
            ip: insn.ip(),
            bytes: insn_bytes,
            text: output.clone(),
        });
    }

    result
}

/// Скомпилировать файл LLVM IR (.ll) во временный объектник (.obj) и извлечь опкоды
pub fn extract_opcodes_from_ll(
    ll_path: &Path,
    clang_path: &str,
    opt_level: &str,
    target: &str,
) -> Result<OpcodesReport, String> {
    let obj_path = ll_path.with_extension("opcodes.obj");

    let mut cmd = Command::new(clang_path);
    cmd.arg("-c");
    cmd.arg(format!("--target={target}"));
    cmd.arg(format!("-O{opt_level}"));
    // Подавляем секции исключений/unwind для максимально чистых опкодов
    cmd.arg("-fno-asynchronous-unwind-tables");
    cmd.arg("-fno-exceptions");
    cmd.arg("-ffunction-sections");
    cmd.arg(ll_path);
    cmd.arg("-o").arg(&obj_path);

    let output = cmd.output().map_err(|e| format!("ошибка запуска clang `{clang_path}`: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ошибка компиляции clang в объектный файл:\n{err}"));
    }

    let obj_bytes = std::fs::read(&obj_path)
        .map_err(|e| format!("не удалось прочитать сгенерированный `{}`: {e}", obj_path.display()))?;

    let _ = std::fs::remove_file(&obj_path);

    extract_opcodes_from_obj(&obj_bytes)
}

/// Скомпилировать исходный код Goraw в строку LLVM IR
pub fn compile_gw_to_ll_str(file: &str, src: &str, target_triple: &str) -> Result<String, String> {
    let mut diags = crate::diag::Diags::new(file, src);
    let toks = crate::lexer::Lexer::new(src).tokenize(&mut diags);
    let mut prog = crate::parser::Parser::new(toks, src, &mut diags).parse_program();
    if diags.has_errors() {
        return Err(diags.render_human());
    }

    crate::mono::monomorphize(&mut prog);

    let mut collected = Vec::new();
    let ctx = crate::types::collect(&prog.structs, &prog.enums, &prog.fns, &mut collected);
    for d in collected {
        diags.push(d);
    }
    if diags.has_errors() {
        return Err(diags.render_human());
    }

    let cg = crate::codegen::Codegen::new(&ctx, &mut diags)
        .with_target_triple(target_triple.to_string());
    let ir = cg.emit_module(&prog);

    if diags.has_errors() {
        return Err(diags.render_human());
    }

    Ok(ir)
}

/// Скомпилировать файл Goraw (.gw) и извлечь опкоды
pub fn extract_opcodes_from_gw(
    gw_path: &Path,
    clang_path: &str,
    opt_level: &str,
    target: &str,
) -> Result<OpcodesReport, String> {
    let src = std::fs::read_to_string(gw_path)
        .map_err(|e| format!("не удалось прочитать `{}`: {e}", gw_path.display()))?;
    let file_str = gw_path.display().to_string();
    let ll_ir = compile_gw_to_ll_str(&file_str, &src, target)?;

    let temp_ll = gw_path.with_extension("opcodes_temp.ll");
    std::fs::write(&temp_ll, ll_ir)
        .map_err(|e| format!("не удалось записать `{}`: {e}", temp_ll.display()))?;

    let res = extract_opcodes_from_ll(&temp_ll, clang_path, opt_level, target);
    let _ = std::fs::remove_file(&temp_ll);
    res
}

/// Скомпилировать файл ассемблера (.asm) и извлечь опкоды
pub fn extract_opcodes_from_asm(asm_path: &Path) -> Result<OpcodesReport, String> {
    let src = std::fs::read_to_string(asm_path)
        .map_err(|e| format!("не удалось прочитать `{}`: {e}", asm_path.display()))?;
    let file_str = asm_path.display().to_string();
    let (obj_opt, diags) = crate::asm::assemble(&file_str, &src);
    if diags.has_errors() {
        return Err(diags.render_human());
    }
    let obj_bytes = obj_opt.ok_or_else(|| "ассемблирование не вернуло объектный код".to_string())?;
    extract_opcodes_from_obj(&obj_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disassemble_nop_ret() {
        // nop (0x90), ret (0xc3)
        let bytes = vec![0x90, 0xc3];
        let insns = disassemble_bytes(&bytes, 0x1000);
        assert_eq!(insns.len(), 2);
        assert_eq!(insns[0].text, "nop");
        assert_eq!(insns[0].bytes, vec![0x90]);
        assert_eq!(insns[1].text, "ret");
        assert_eq!(insns[1].bytes, vec![0xc3]);
    }

    #[test]
    fn test_format_render_goraw_and_c() {
        let bytes = vec![0x48, 0x31, 0xc0, 0xc3]; // xor rax, rax; ret
        let insns = disassemble_bytes(&bytes, 0);
        let report = OpcodesReport {
            functions: vec![FunctionOpcodes {
                name: "test_zero".to_string(),
                offset: 0,
                bytes: bytes.clone(),
                instructions: insns,
            }],
            all_text: bytes,
        };

        let gw_bytes = report.render(OpcodeFormat::Goraw, None).unwrap();
        let gw_str = String::from_utf8(gw_bytes).unwrap();
        assert!(gw_str.contains("const TEST_ZERO_OPCODES: [u8; 4] = ["));
        assert!(gw_str.contains("0x48, 0x31, 0xc0, 0xc3"));

        let c_bytes = report.render(OpcodeFormat::C, None).unwrap();
        let c_str = String::from_utf8(c_bytes).unwrap();
        assert!(c_str.contains("const unsigned char test_zero_opcodes[4] = {"));

        let hex_bytes = report.render(OpcodeFormat::Hex, None).unwrap();
        let hex_str = String::from_utf8(hex_bytes).unwrap();
        assert!(hex_str.contains("\\x48\\x31\\xc0\\xc3"));
    }
}
