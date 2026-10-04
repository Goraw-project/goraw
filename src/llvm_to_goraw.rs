//! Транслятор LLVM IR (.ll) в Goraw (.gw).
//!
//! Данный модуль выполняет декомпиляцию / лифтинг текстового представления
//! LLVM IR в идиоматичный, валидный исходный код Goraw с полноценным
//! восстановлением структурированного потока управления (Relooper / CFG structuring),
//! элиминацией типов нулевого размера (ZST), изоляцией интринсиков LLVM
//! и единой областью видимости для SSA-переменных.

use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct LlvmToGorawOptions {
    pub verbose: bool,
    pub emit_comments: bool,
    pub structured_cfg: bool,
}

impl Default for LlvmToGorawOptions {
    fn default() -> Self {
        Self {
            verbose: false,
            emit_comments: true,
            structured_cfg: true,
        }
    }
}

/// Основная точка входа: транслирует текстовый LLVM IR в исходный код Goraw.
pub fn transpile_llvm_ir(ir_text: &str, opts: &LlvmToGorawOptions) -> Result<String, String> {
    let mut parser = LlvmParser::new(ir_text);
    let module = parser.parse_module()?;
    let codegen = GorawLifter::new(&module, opts);
    Ok(codegen.lift_module())
}

/// Транслирует .ll файл на диске в код Goraw.
pub fn transpile_llvm_file(path: &Path, opts: &LlvmToGorawOptions) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("не удалось прочитать `{}`: {e}", path.display()))?;
    transpile_llvm_ir(&text, opts)
}

// ============================================================================
// Представление LLVM IR (AST)
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
pub enum LlvmType {
    Void,
    I1,
    I8,
    I16,
    I32,
    I64,
    I128,
    Float,
    Double,
    Ptr,
    Array(usize, Box<LlvmType>),
    Struct(String, Vec<LlvmType>),
    AnonStruct(Vec<LlvmType>),
    Ident(String),
}

impl LlvmType {
    pub fn to_goraw_type(&self) -> String {
        match self {
            LlvmType::Void => "void".to_string(),
            LlvmType::I1 => "bool".to_string(),
            LlvmType::I8 => "i8".to_string(),
            LlvmType::I16 => "i16".to_string(),
            LlvmType::I32 => "i32".to_string(),
            LlvmType::I64 => "i64".to_string(),
            LlvmType::I128 => "i128".to_string(),
            LlvmType::Float => "f32".to_string(),
            LlvmType::Double => "f64".to_string(),
            LlvmType::Ptr => "*u8".to_string(),
            LlvmType::Array(n, elem) => format!("[{}]{}", n, elem.to_goraw_type()),
            LlvmType::Struct(name, _) => sanitize_type_name(name),
            LlvmType::AnonStruct(fields) => {
                let tags: Vec<String> = fields.iter().map(|f| f.to_goraw_type()).collect();
                format!("Anon_{}", tags.join("_")).replace('*', "ptr_")
            }
            LlvmType::Ident(name) => sanitize_type_name(name),
        }
    }

    pub fn collect_anon_structs(&self, acc: &mut Vec<Vec<LlvmType>>) {
        match self {
            LlvmType::AnonStruct(fields) => {
                if !acc.contains(fields) {
                    acc.push(fields.clone());
                }
                for f in fields {
                    f.collect_anon_structs(acc);
                }
            }
            LlvmType::Array(_, elem) => elem.collect_anon_structs(acc),
            _ => {}
        }
    }

    pub fn default_zero_val(&self, structs: &[StructDef]) -> String {
        match self {
            LlvmType::Void => "()".to_string(),
            LlvmType::I1 => "false".to_string(),
            LlvmType::I8 | LlvmType::I16 | LlvmType::I32 | LlvmType::I64 | LlvmType::I128 => "0".to_string(),
            LlvmType::Float => "0.0".to_string(),
            LlvmType::Double => "0.0".to_string(),
            LlvmType::Ptr => "null".to_string(),
            LlvmType::Array(_, _) => "[]".to_string(),
            LlvmType::Struct(name, fields) => {
                let sname = sanitize_type_name(name);
                let field_inits: Vec<String> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| format!("field{i}: {}", f.default_zero_val(structs)))
                    .collect();
                format!("{} {{ {} }}", sname, field_inits.join(", "))
            }
            LlvmType::AnonStruct(fields) => {
                let sname = self.to_goraw_type();
                let field_inits: Vec<String> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| format!("field{i}: {}", f.default_zero_val(structs)))
                    .collect();
                format!("{} {{ {} }}", sname, field_inits.join(", "))
            }
            LlvmType::Ident(name) => {
                let sname = sanitize_type_name(name);
                if let Some(sdef) = structs.iter().find(|s| sanitize_type_name(&s.name) == sname) {
                    let field_inits: Vec<String> = sdef
                        .fields
                        .iter()
                        .enumerate()
                        .map(|(i, f)| format!("field{i}: {}", f.default_zero_val(structs)))
                        .collect();
                    format!("{} {{ {} }}", sname, field_inits.join(", "))
                } else {
                    format!("{} {{}}", sname)
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LlvmValue {
    Reg(String),
    Global(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Undef,
    StringLit(String),
    IntToPtr(Box<LlvmValue>),
    GepExpr {
        base: Box<LlvmValue>,
        offset: Box<LlvmValue>,
    },
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<LlvmType>,
}

#[derive(Clone, Debug)]
pub struct GlobalVarDef {
    pub name: String,
    pub ty: LlvmType,
    pub is_constant: bool,
    pub init_val: Option<LlvmValue>,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: String,
    pub ret_ty: LlvmType,
    pub params: Vec<(LlvmType, String)>,
    pub is_vararg: bool,
}

#[derive(Clone, Debug)]
pub struct FnDef {
    pub name: String,
    pub ret_ty: LlvmType,
    pub params: Vec<(LlvmType, String)>,
    pub is_vararg: bool,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Clone, Debug)]
pub struct BasicBlock {
    pub label: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

#[derive(Clone, Debug)]
pub enum Instruction {
    Nop,
    Alloca {
        res: String,
        ty: LlvmType,
    },
    Store {
        val: LlvmValue,
        ptr: LlvmValue,
    },
    Load {
        res: String,
        ty: LlvmType,
        ptr: LlvmValue,
    },
    BinOp {
        res: String,
        op: String,
        ty: LlvmType,
        lhs: LlvmValue,
        rhs: LlvmValue,
    },
    Neg {
        res: String,
        ty: LlvmType,
        val: LlvmValue,
    },
    ICmp {
        res: String,
        cond: String,
        lhs: LlvmValue,
        rhs: LlvmValue,
    },
    FCmp {
        res: String,
        cond: String,
        lhs: LlvmValue,
        rhs: LlvmValue,
    },
    Cast {
        res: String,
        dest_ty: LlvmType,
        val: LlvmValue,
    },
    Gep {
        res: String,
        elem_ty: LlvmType,
        ptr: LlvmValue,
        indices: Vec<LlvmValue>,
    },
    Call {
        res: Option<String>,
        ret_ty: LlvmType,
        callee: String,
        args: Vec<LlvmValue>,
    },
    Select {
        res: String,
        ty: LlvmType,
        cond: LlvmValue,
        val_true: LlvmValue,
        val_false: LlvmValue,
    },
    Phi {
        res: String,
        ty: LlvmType,
        incoming: Vec<(LlvmValue, String)>,
    },
    IfThen {
        cond: LlvmValue,
        then_insts: Vec<Instruction>,
    },
    IfElse {
        cond: LlvmValue,
        then_insts: Vec<Instruction>,
        else_insts: Vec<Instruction>,
    },
    IfThenRet {
        cond: LlvmValue,
        then_insts: Vec<Instruction>,
        ret_val: Option<LlvmValue>,
    },
    While {
        cond: LlvmValue,
        body_insts: Vec<Instruction>,
    },
}

#[derive(Clone, Debug)]
pub enum Terminator {
    Ret(Option<LlvmValue>),
    Br(String),
    CondBr {
        cond: LlvmValue,
        then_label: String,
        else_label: String,
    },
    Switch {
        val: LlvmValue,
        default_label: String,
        cases: Vec<(LlvmValue, String)>,
    },
    Unreachable,
}

#[derive(Default, Debug)]
pub struct Module {
    pub structs: Vec<StructDef>,
    pub globals: Vec<GlobalVarDef>,
    pub string_constants: HashMap<String, String>,
    pub declares: Vec<FnDecl>,
    pub functions: Vec<FnDef>,
}

// ============================================================================
// Парсер текстового LLVM IR
// ============================================================================

struct LlvmParser<'a> {
    lines: Vec<&'a str>,
    line_idx: usize,
}

impl<'a> LlvmParser<'a> {
    fn new(input: &'a str) -> Self {
        let lines: Vec<&'a str> = input.lines().collect();
        Self {
            lines,
            line_idx: 0,
        }
    }

    fn parse_module(&mut self) -> Result<Module, String> {
        let mut module = Module::default();

        while self.line_idx < self.lines.len() {
            let line = self.lines[self.line_idx].trim();
            self.line_idx += 1;

            if line.is_empty() || line.starts_with(';') {
                continue;
            }

            // Игнорируем метаданные и директивы модуля
            if line.starts_with("target ")
                || line.starts_with("source_filename ")
                || line.starts_with("attributes ")
                || line.starts_with('!')
            {
                continue;
            }

            // Определение структуры: %struct.Foo = type { ... }
            if line.starts_with('%') && line.contains(" = type ") {
                if let Some(s) = self.parse_struct_line(line) {
                    module.structs.push(s);
                }
                continue;
            }

            // Глобальные переменные или константы: @name = ...
            if line.starts_with('@') && line.contains('=') {
                if let Some((name, sval)) = self.parse_string_constant(line) {
                    module.string_constants.insert(name, sval);
                } else if let Some(g) = self.parse_global_line(line) {
                    module.globals.push(g);
                }
                continue;
            }

            // Объявление функции: declare ...
            if line.starts_with("declare ") {
                if let Some(d) = self.parse_declare_line(line) {
                    module.declares.push(d);
                }
                continue;
            }

            // Определение функции: define ...
            if line.starts_with("define ") {
                let f = self.parse_function(line)?;
                module.functions.push(f);
                continue;
            }
        }

        Ok(module)
    }

    fn parse_struct_line(&self, line: &str) -> Option<StructDef> {
        let parts: Vec<&str> = line.split(" = type ").collect();
        if parts.len() != 2 {
            return None;
        }
        let name = parts[0].trim();
        let body = parts[1].trim().trim_start_matches('<').trim_end_matches('>');
        let body = body.trim_start_matches('{').trim_end_matches('}').trim();
        let mut fields = Vec::new();
        if !body.is_empty() {
            for item in split_top_level_comma(body) {
                fields.push(parse_type_str(item.trim()));
            }
        }
        Some(StructDef {
            name: name.to_string(),
            fields,
        })
    }

    fn parse_string_constant(&self, line: &str) -> Option<(String, String)> {
        let eq_idx = line.find('=')?;
        let name = line[..eq_idx].trim().to_string();

        // 1. C string literal: c"..."
        if line.contains("c\"") {
            let start_str = line.find("c\"")? + 2;
            let end_str = line[start_str..].rfind('"')? + start_str;
            let raw_c_str = &line[start_str..end_str];
            let decoded = decode_llvm_c_string(raw_c_str);
            return Some((name, decoded));
        }

        // 2. Wide string literal: [N x i16] [i16 112, i16 58, ...]
        if line.contains("x i16] [") || line.contains("] [i16 ") {
            if let Some(bracket_start) = line.rfind('[') {
                if let Some(bracket_end) = line.rfind(']') {
                    if bracket_start < bracket_end {
                        let elements = &line[bracket_start + 1..bracket_end];
                        let mut s = String::new();
                        for item in elements.split(',') {
                            let item = item.trim();
                            if let Some(num_str) = item.strip_prefix("i16 ") {
                                if let Ok(val) = num_str.trim().parse::<u16>() {
                                    if val == 0 {
                                        break; // null terminator
                                    }
                                    if let Some(ch) = char::from_u32(val as u32) {
                                        s.push(ch);
                                    }
                                }
                            }
                        }
                        if !s.is_empty() {
                            return Some((name, s));
                        }
                    }
                }
            }
        }

        // 3. Byte array string literal: [N x i8] [i8 65, i8 66, ...]
        if line.contains("x i8] [") || line.contains("] [i8 ") {
            if let Some(bracket_start) = line.rfind('[') {
                if let Some(bracket_end) = line.rfind(']') {
                    if bracket_start < bracket_end {
                        let elements = &line[bracket_start + 1..bracket_end];
                        let mut s = String::new();
                        for item in elements.split(',') {
                            let item = item.trim();
                            if let Some(num_str) = item.strip_prefix("i8 ") {
                                if let Ok(val) = num_str.trim().parse::<u8>() {
                                    if val == 0 {
                                        break;
                                    }
                                    s.push(val as char);
                                }
                            }
                        }
                        if !s.is_empty() {
                            return Some((name, s));
                        }
                    }
                }
            }
        }

        None
    }

    fn parse_global_line(&self, line: &str) -> Option<GlobalVarDef> {
        let eq_idx = line.find('=')?;
        let name = line[..eq_idx].trim().to_string();
        let rest = line[eq_idx + 1..].trim();

        let is_constant = rest.contains(" constant ");
        let is_global = rest.contains(" global ");
        if !is_constant && !is_global {
            return None;
        }

        let keyword = if is_constant { " constant " } else { " global " };
        let parts: Vec<&str> = rest.splitn(2, keyword).collect();
        if parts.len() != 2 {
            return None;
        }
        let type_and_val = parts[1].trim();
        let (ty, after_ty) = parse_type_and_rest(type_and_val);

        let val_clean = if let Some(idx) = after_ty.find(", align") {
            after_ty[..idx].trim()
        } else if let Some(idx) = after_ty.find(", section") {
            after_ty[..idx].trim()
        } else {
            after_ty.trim()
        };

        let init_val = if val_clean.is_empty() || val_clean == "undef" || val_clean.starts_with('[') || val_clean.starts_with('{') {
            None
        } else if val_clean == "zeroinitializer" {
            Some(LlvmValue::Int(0))
        } else {
            Some(parse_value_str(val_clean))
        };

        Some(GlobalVarDef {
            name,
            ty,
            is_constant,
            init_val,
        })
    }

    fn parse_declare_line(&self, line: &str) -> Option<FnDecl> {
        let line = line.trim_start_matches("declare ").trim();
        let at_idx = line.find('@')?;
        let pre_at = line[..at_idx].trim();
        let ret_ty = parse_return_type_from_header(pre_at);

        let paren_start = line[at_idx..].find('(').map(|i| at_idx + i)?;
        let paren_end = line.rfind(')')?;
        let name = line[at_idx..paren_start].trim().to_string();

        let params_part = &line[paren_start + 1..paren_end];
        let mut params = Vec::new();
        let mut is_vararg = false;

        for p in split_top_level_comma(params_part) {
            let p = p.trim();
            if p == "..." {
                is_vararg = true;
                continue;
            }
            if p.is_empty() {
                continue;
            }
            let words: Vec<&str> = p.split_whitespace().collect();
            let ty = parse_type_str(words[0]);
            let pname = if words.len() > 1 && words[1].starts_with('%') {
                words[1].to_string()
            } else {
                format!("arg{}", params.len())
            };
            params.push((ty, pname));
        }

        Some(FnDecl {
            name,
            ret_ty,
            params,
            is_vararg,
        })
    }

    fn parse_function(&mut self, header_line: &str) -> Result<FnDef, String> {
        let at_idx = header_line.find('@').ok_or("в функции пропущен '@'")?;
        let pre_at = header_line[..at_idx].trim();
        let ret_ty = parse_return_type_from_header(pre_at);

        let paren_start = header_line[at_idx..].find('(').map(|i| at_idx + i).ok_or("в функции пропущен '('")?;
        let paren_end = header_line.rfind(')').ok_or("в функции пропущен ')'")?;
        let name = header_line[at_idx..paren_start].trim().to_string();

        let params_part = &header_line[paren_start + 1..paren_end];
        let mut params = Vec::new();
        let mut is_vararg = false;

        for (idx, p) in split_top_level_comma(params_part).iter().enumerate() {
            let p = p.trim();
            if p == "..." {
                is_vararg = true;
                continue;
            }
            if p.is_empty() {
                continue;
            }
            let words: Vec<&str> = p.split_whitespace().collect();
            let ty = parse_type_str(words[0]);
            let pname = words
                .iter()
                .rev()
                .find(|w| w.starts_with('%'))
                .cloned()
                .unwrap_or("")
                .to_string();
            let pname = if pname.is_empty() {
                format!("arg{idx}")
            } else {
                pname
            };
            params.push((ty, pname));
        }

        // Парсим блоки до '}'
        let mut blocks = Vec::new();
        let mut cur_label = "entry".to_string();
        let mut cur_insts = Vec::new();

        while self.line_idx < self.lines.len() {
            let mut line = self.lines[self.line_idx].trim();
            self.line_idx += 1;

            if let Some(c_idx) = line.find(';') {
                line = line[..c_idx].trim();
            }

            if line.is_empty() {
                continue;
            }

            if line == "}" {
                if !cur_insts.is_empty() {
                    blocks.push(BasicBlock {
                        label: cur_label,
                        instructions: cur_insts,
                        terminator: Terminator::Ret(None),
                    });
                }
                break;
            }

            // Метка базового блока: name: или 5:
            if line.ends_with(':') && !line.contains('=') {
                let raw_label = line.trim_end_matches(':').trim();
                if !cur_insts.is_empty() {
                    blocks.push(BasicBlock {
                        label: cur_label.clone(),
                        instructions: cur_insts,
                        terminator: Terminator::Br(raw_label.to_string()),
                    });
                    cur_insts = Vec::new();
                }
                cur_label = raw_label.to_string();
                continue;
            }

            // Инструкции терминаторов
            if line.starts_with("ret ") {
                let term = parse_ret_terminator(line);
                blocks.push(BasicBlock {
                    label: cur_label.clone(),
                    instructions: cur_insts,
                    terminator: term,
                });
                cur_insts = Vec::new();
                cur_label = format!("bb_{}", blocks.len());
                continue;
            } else if line.starts_with("br ") {
                let term = parse_br_terminator(line);
                blocks.push(BasicBlock {
                    label: cur_label.clone(),
                    instructions: cur_insts,
                    terminator: term,
                });
                cur_insts = Vec::new();
                cur_label = format!("bb_{}", blocks.len());
                continue;
            } else if line.starts_with("switch ") {
                let term = parse_switch_terminator(line);
                blocks.push(BasicBlock {
                    label: cur_label.clone(),
                    instructions: cur_insts,
                    terminator: term,
                });
                cur_insts = Vec::new();
                cur_label = format!("bb_{}", blocks.len());
                continue;
            } else if line.starts_with("unreachable") || line.starts_with("cleanupret ") || line.starts_with("catchret ") || line.starts_with("resume ") {
                blocks.push(BasicBlock {
                    label: cur_label.clone(),
                    instructions: cur_insts,
                    terminator: Terminator::Unreachable,
                });
                cur_insts = Vec::new();
                cur_label = format!("bb_{}", blocks.len());
                continue;
            }

            // Обычная инструкция
            if let Some(inst) = parse_instruction_line(line) {
                if !matches!(inst, Instruction::Nop) {
                    cur_insts.push(inst);
                }
            }
        }

        Ok(FnDef {
            name,
            ret_ty,
            params,
            is_vararg,
            blocks,
        })
    }
}

fn parse_return_type_from_header(pre_at: &str) -> LlvmType {
    let pre_at = pre_at.trim();
    if pre_at.ends_with('}') {
        if let Some(b_start) = pre_at.rfind('{') {
            return parse_type_str(&pre_at[b_start..]);
        }
    }
    if pre_at.ends_with(']') {
        if let Some(b_start) = pre_at.rfind('[') {
            return parse_type_str(&pre_at[b_start..]);
        }
    }
    let words: Vec<&str> = pre_at.split_whitespace().collect();
    let ret_str = words.last().cloned().unwrap_or("void");
    parse_type_str(ret_str)
}

fn parse_ret_terminator(line: &str) -> Terminator {
    let rest = line.trim_start_matches("ret ").trim();
    if rest == "void" || rest.is_empty() {
        Terminator::Ret(None)
    } else {
        let parts = split_top_level_comma(rest);
        if parts.len() >= 2 {
            let last_part = parts.last().unwrap().trim();
            let words: Vec<&str> = last_part.split_whitespace().collect();
            Terminator::Ret(Some(parse_value_str(words.last().unwrap_or(&"0"))))
        } else {
            let words: Vec<&str> = rest.split_whitespace().collect();
            if words.len() >= 2 {
                Terminator::Ret(Some(parse_value_str(words[1])))
            } else {
                Terminator::Ret(Some(parse_value_str(words[0])))
            }
        }
    }
}

fn parse_br_terminator(line: &str) -> Terminator {
    let rest = line.trim_start_matches("br ").trim();
    if rest.starts_with("label %") {
        let target = rest.trim_start_matches("label %").trim();
        let target = target.split(',').next().unwrap_or(target).trim();
        let target = target.split_whitespace().next().unwrap_or(target).trim();
        Terminator::Br(target.to_string())
    } else if rest.starts_with("i1 ") {
        let parts = split_top_level_comma(rest);
        if parts.len() >= 3 {
            let cond_part = parts[0].trim().trim_start_matches("i1 ").trim();
            let then_part = parts[1].trim().trim_start_matches("label %").trim();
            let then_part = then_part.split_whitespace().next().unwrap_or(then_part).trim();
            let else_part = parts[2].trim().trim_start_matches("label %").trim();
            let else_part = else_part.split_whitespace().next().unwrap_or(else_part).trim();
            Terminator::CondBr {
                cond: parse_value_str(cond_part),
                then_label: then_part.to_string(),
                else_label: else_part.to_string(),
            }
        } else {
            Terminator::Unreachable
        }
    } else {
        Terminator::Unreachable
    }
}

fn parse_switch_terminator(line: &str) -> Terminator {
    let rest = line.trim_start_matches("switch ").trim();
    let bracket_start = rest.find('[');
    let def_label = if let Some(b_idx) = bracket_start {
        let pre_b = &rest[..b_idx];
        pre_b
            .split("label %")
            .nth(1)
            .map(|s| s.trim().trim_end_matches(',').trim().to_string())
            .unwrap_or_else(|| "default".to_string())
    } else {
        "default".to_string()
    };

    let val = rest
        .split(',')
        .next()
        .map(|s| {
            let words: Vec<&str> = s.split_whitespace().collect();
            if words.len() >= 2 {
                parse_value_str(words[1])
            } else {
                parse_value_str(words[0])
            }
        })
        .unwrap_or(LlvmValue::Int(0));

    let mut cases = Vec::new();
    if let (Some(b_start), Some(b_end)) = (rest.find('['), rest.rfind(']')) {
        let body = &rest[b_start + 1..b_end].trim();
        let tokens: Vec<&str> = body.split_whitespace().collect();
        let mut i = 0;
        while i + 3 < tokens.len() {
            let cval = parse_value_str(tokens[i + 1].trim_end_matches(','));
            let clabel = tokens[i + 3].trim_start_matches('%').trim();
            cases.push((cval, clabel.to_string()));
            i += 4;
        }
    }

    Terminator::Switch {
        val,
        default_label: def_label,
        cases,
    }
}

fn parse_instruction_line(line: &str) -> Option<Instruction> {
    let line = line.trim();
    let clean_line = if let Some(idx) = line.find(", !") {
        &line[..idx]
    } else {
        line
    };

    // Пропускаем интринсики и инструкции исключений
    if clean_line.contains("cleanuppad ")
        || clean_line.contains("catchpad ")
        || clean_line.contains("landingpad ")
        || clean_line.contains("catchswitch ")
    {
        return Some(Instruction::Nop);
    }

    // Все инструкции с присваиванием: %res = ...
    if let Some(eq_idx) = clean_line.find('=') {
        let res = clean_line[..eq_idx].trim().to_string();
        let rhs = clean_line[eq_idx + 1..].trim();

        // 1. alloca
        if rhs.starts_with("alloca ") {
            let rest = rhs.trim_start_matches("alloca ").trim();
            let ty_str = rest.split(',').next().unwrap_or("i32").trim();
            return Some(Instruction::Alloca {
                res,
                ty: parse_type_str(ty_str),
            });
        }

        // 2. load
        if rhs.starts_with("load ") {
            let rest = rhs.trim_start_matches("load ").trim();
            let parts = split_top_level_comma(rest);
            let ty_str = parts.get(0).unwrap_or(&"i32").trim();
            let ptr_str = parts.get(1).map(|s| extract_ptr_expr(s)).unwrap_or("");
            return Some(Instruction::Load {
                res,
                ty: parse_type_str(ty_str),
                ptr: parse_value_str(ptr_str),
            });
        }

        // 3. fneg
        if rhs.starts_with("fneg ") {
            let rest = rhs.trim_start_matches("fneg ").trim();
            let words: Vec<&str> = rest.split_whitespace().collect();
            let ty = if words.len() >= 2 {
                parse_type_str(words[0])
            } else {
                LlvmType::Double
            };
            let val = words.last().unwrap_or(&"0");
            return Some(Instruction::Neg {
                res,
                ty,
                val: parse_value_str(val),
            });
        }

        // 4. icmp / fcmp
        if rhs.starts_with("icmp ") || rhs.starts_with("fcmp ") {
            let is_f = rhs.starts_with("fcmp ");
            let rest = if is_f {
                rhs.trim_start_matches("fcmp ").trim()
            } else {
                rhs.trim_start_matches("icmp ").trim()
            };
            let parts = split_top_level_comma(rest);
            if parts.len() >= 2 {
                let cond_and_lhs = parts[0].trim();
                let rhs_str = parts[1].split_whitespace().last().unwrap_or("0");
                let cl_words: Vec<&str> = cond_and_lhs.split_whitespace().collect();
                if cl_words.len() >= 2 {
                    let cond = cl_words[0].to_string();
                    let lhs = parse_value_str(cl_words.last().unwrap());
                    let rhs = parse_value_str(rhs_str);
                    return if is_f {
                        Some(Instruction::FCmp { res, cond, lhs, rhs })
                    } else {
                        Some(Instruction::ICmp { res, cond, lhs, rhs })
                    };
                }
            }
        }

        // 5. cast ops
        let cast_keywords = [
            "trunc", "zext", "sext", "fptrunc", "fpext", "fptoui", "fptosi", "uitofp", "sitofp",
            "ptrtoint", "inttoptr", "bitcast",
        ];
        for kw in &cast_keywords {
            if rhs.starts_with(kw) {
                let rest = rhs.trim_start_matches(kw).trim();
                let to_parts: Vec<&str> = rest.split(" to ").collect();
                if to_parts.len() == 2 {
                    let src_words: Vec<&str> = to_parts[0].split_whitespace().collect();
                    let src_val = parse_value_str(src_words.last().unwrap_or(&"0"));
                    let dest_ty = parse_type_str(to_parts[1].trim());
                    return Some(Instruction::Cast {
                        res,
                        dest_ty,
                        val: src_val,
                    });
                }
            }
        }

        // 6. getelementptr
        if rhs.starts_with("getelementptr ") {
            return parse_gep_instruction(&res, rhs);
        }

        // 7. select
        if rhs.starts_with("select ") {
            let rest = rhs.trim_start_matches("select ").trim();
            let parts = split_top_level_comma(rest);
            if parts.len() == 3 {
                let cond = parse_value_str(parts[0].split_whitespace().last().unwrap_or(""));
                let val_part = parts[1].trim();
                let ty = if let Some(space_idx) = val_part.find(' ') {
                    parse_type_str(&val_part[..space_idx])
                } else {
                    LlvmType::I64
                };
                let val_true = parse_value_str(parts[1].split_whitespace().last().unwrap_or(""));
                let val_false = parse_value_str(parts[2].split_whitespace().last().unwrap_or(""));
                return Some(Instruction::Select {
                    res,
                    ty,
                    cond,
                    val_true,
                    val_false,
                });
            }
        }

        // insertelement: %broadcast.splatinsert = insertelement <16 x i8> poison, i8 %_13, i64 0
        if rhs.starts_with("insertelement ") {
            let rest = rhs.trim_start_matches("insertelement ").trim();
            let parts = split_top_level_comma(rest);
            if parts.len() >= 2 {
                let elem_str = parts[1].split_whitespace().last().unwrap_or("0");
                return Some(Instruction::Cast {
                    res,
                    dest_ty: LlvmType::I64,
                    val: parse_value_str(elem_str),
                });
            }
        }

        // shufflevector: %broadcast.splat = shufflevector <16 x i8> %broadcast.splatinsert, <16 x i8> poison, <16 x i32> zeroinitializer
        if rhs.starts_with("shufflevector ") {
            let rest = rhs.trim_start_matches("shufflevector ").trim();
            let parts = split_top_level_comma(rest);
            if let Some(first) = parts.first() {
                let val_str = first.split_whitespace().last().unwrap_or("0");
                return Some(Instruction::Cast {
                    res,
                    dest_ty: LlvmType::I64,
                    val: parse_value_str(val_str),
                });
            }
        }

        // extractelement: %res = extractelement <16 x i8> %vec, i64 %idx
        if rhs.starts_with("extractelement ") {
            let rest = rhs.trim_start_matches("extractelement ").trim();
            let parts = split_top_level_comma(rest);
            if let Some(first) = parts.first() {
                let val_str = first.split_whitespace().last().unwrap_or("0");
                return Some(Instruction::Cast {
                    res,
                    dest_ty: LlvmType::I64,
                    val: parse_value_str(val_str),
                });
            }
        }

        // 8. phi
        if rhs.starts_with("phi ") {
            let rest = rhs.trim_start_matches("phi ").trim();
            let ty_str = rest.split_whitespace().next().unwrap_or("i32");
            let mut incoming = Vec::new();
            for block_part in rest.split('[') {
                if let Some(c_idx) = block_part.find(']') {
                    let inner = &block_part[..c_idx].trim();
                    let pair: Vec<&str> = inner.split(',').collect();
                    if pair.len() == 2 {
                        let val = parse_value_str(pair[0].trim());
                        let blk = pair[1].trim().trim_start_matches('%').to_string();
                        incoming.push((val, blk));
                    }
                }
            }
            return Some(Instruction::Phi {
                res,
                ty: parse_type_str(ty_str),
                incoming,
            });
        }

        // 9. call
        if rhs.contains("call ") {
            return parse_call_instruction(Some(res), rhs);
        }

        // 10. binary ops
        let bin_ops = [
            "add", "sub", "mul", "sdiv", "udiv", "srem", "urem", "fadd", "fsub", "fmul", "fdiv",
            "shl", "lshr", "ashr", "and", "or", "xor",
        ];
        for op in &bin_ops {
            if rhs.starts_with(op) {
                let rest = rhs.trim_start_matches(op).trim();
                let rest = rest
                    .trim_start_matches("nsw ")
                    .trim_start_matches("nuw ")
                    .trim_start_matches("exact ")
                    .trim_start_matches("fast ")
                    .trim();
                let parts = split_top_level_comma(rest);
                if parts.len() >= 2 {
                    let first_part = parts[0].trim();
                    let ty = if let Some(space_idx) = first_part.find(' ') {
                        parse_type_str(&first_part[..space_idx])
                    } else if op.starts_with('f') {
                        LlvmType::Double
                    } else {
                        LlvmType::I64
                    };
                    let lhs_str = first_part.split_whitespace().last().unwrap_or("0");
                    let rhs_str = parts[1].split_whitespace().last().unwrap_or("0");
                    return Some(Instruction::BinOp {
                        res,
                        op: op.to_string(),
                        ty,
                        lhs: parse_value_str(lhs_str),
                        rhs: parse_value_str(rhs_str),
                    });
                }
            }
        }
    } else {
        // Инструкции без присваивания
        // 1. store
        if clean_line.starts_with("store ") {
            let rest = clean_line.trim_start_matches("store ").trim();
            let parts = split_top_level_comma(rest);
            if parts.len() >= 2 {
                let val_part = parts[0].trim();
                let val_str = if let Some(space_idx) = val_part.find(' ') {
                    val_part[space_idx + 1..].trim()
                } else {
                    val_part
                };
                let ptr_str = extract_ptr_expr(parts[1]);
                return Some(Instruction::Store {
                    val: parse_value_str(val_str),
                    ptr: parse_value_str(ptr_str),
                });
            }
        }

        // 2. call void
        if clean_line.starts_with("call ") || clean_line.contains(" call ") {
            return parse_call_instruction(None, clean_line);
        }
    }

    None
}

fn extract_ptr_expr(s: &str) -> &str {
    let mut s = s.trim();
    if let Some(idx) = s.find(", align") {
        s = s[..idx].trim();
    } else if let Some(idx) = s.find(" align ") {
        s = s[..idx].trim();
    }
    if let Some(idx) = s.find(", !") {
        s = s[..idx].trim();
    } else if let Some(idx) = s.find(" !") {
        s = s[..idx].trim();
    }
    if s.starts_with("ptr ") {
        s = s[4..].trim();
    } else if let Some(space_idx) = s.find(' ') {
        if !s.starts_with("getelementptr") {
            s = s[space_idx + 1..].trim();
        }
    }
    s
}

fn parse_gep_instruction(res: &str, line: &str) -> Option<Instruction> {
    let rest = line.trim_start_matches("getelementptr ").trim();
    let rest = rest.trim_start_matches("inbounds ").trim();
    let parts = split_top_level_comma(rest);
    if parts.len() < 2 {
        return None;
    }
    let elem_ty = parse_type_str(parts[0].trim());
    let ptr_str = parts[1].split_whitespace().last().unwrap_or("null");
    let mut indices = Vec::new();
    for p in &parts[2..] {
        let idx_str = p.split_whitespace().last().unwrap_or("0");
        indices.push(parse_value_str(idx_str));
    }

    Some(Instruction::Gep {
        res: res.to_string(),
        elem_ty,
        ptr: parse_value_str(ptr_str),
        indices,
    })
}

fn parse_call_instruction(res: Option<String>, line: &str) -> Option<Instruction> {
    let call_idx = line.find("call ")?;
    let after_call = line[call_idx + 5..].trim();
    let at_idx = after_call.find('@')?;
    let pre_at = after_call[..at_idx].trim();

    let ret_ty = if pre_at.starts_with('(') || pre_at.contains('(') {
        let first_word = pre_at.split_whitespace().next().unwrap_or("void");
        parse_type_str(first_word)
    } else {
        let words: Vec<&str> = pre_at.split_whitespace().collect();
        parse_type_str(words.last().unwrap_or(&"void"))
    };

    let paren_start = after_call[at_idx..].find('(')? + at_idx;
    let paren_end = after_call.rfind(')')?;
    let callee = after_call[at_idx..paren_start].trim().to_string();

    let args_part = &after_call[paren_start + 1..paren_end];
    let mut args = Vec::new();
    for a in split_top_level_comma(args_part) {
        let a = a.trim();
        if a.is_empty() || a.starts_with("token ") || a.contains("cleanuppad") || a.contains("catchpad") {
            continue;
        }
        let words: Vec<&str> = a.split_whitespace().collect();
        let arg_val = parse_value_str(words.last().unwrap_or(&"0"));
        args.push(arg_val);
    }

    Some(Instruction::Call {
        res,
        ret_ty,
        callee,
        args,
    })
}

fn parse_type_and_rest(s: &str) -> (LlvmType, &str) {
    let s = s.trim();
    if s.starts_with('[') {
        let mut depth = 0;
        let mut end = 0;
        for (i, c) in s.char_indices() {
            if c == '[' {
                depth += 1;
            } else if c == ']' {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
        }
        if end > 0 {
            return (parse_type_str(&s[..end]), s[end..].trim());
        }
    } else if s.starts_with('{') {
        let mut depth = 0;
        let mut end = 0;
        for (i, c) in s.char_indices() {
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
        }
        if end > 0 {
            return (parse_type_str(&s[..end]), s[end..].trim());
        }
    } else if s.starts_with('<') {
        let mut depth = 0;
        let mut end = 0;
        for (i, c) in s.char_indices() {
            if c == '<' {
                depth += 1;
            } else if c == '>' {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
        }
        if end > 0 {
            return (parse_type_str(&s[..end]), s[end..].trim());
        }
    }
    let mut parts = s.splitn(2, |c: char| c.is_whitespace() || c == ',');
    let first = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    (parse_type_str(first), rest)
}

fn parse_type_str(s: &str) -> LlvmType {
    let s = s.trim();
    if s == "void" {
        LlvmType::Void
    } else if s.starts_with('i') && s.len() > 1 && s[1..].chars().all(|c| c.is_ascii_digit()) {
        let bits: usize = s[1..].parse().unwrap_or(32);
        if bits == 1 {
            LlvmType::I1
        } else if bits <= 8 {
            LlvmType::I8
        } else if bits <= 16 {
            LlvmType::I16
        } else if bits <= 32 {
            LlvmType::I32
        } else if bits <= 64 {
            LlvmType::I64
        } else {
            LlvmType::I128
        }
    } else if s == "float" {
        LlvmType::Float
    } else if s == "double" {
        LlvmType::Double
    } else if s == "ptr" || s.ends_with('*') {
        LlvmType::Ptr
    } else if s.starts_with('{') && s.ends_with('}') {
        let body = &s[1..s.len() - 1].trim();
        let mut fields = Vec::new();
        if !body.is_empty() {
            for item in split_top_level_comma(body) {
                fields.push(parse_type_str(item.trim()));
            }
        }
        LlvmType::AnonStruct(fields)
    } else if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1].trim();
        let parts: Vec<&str> = inner.split(" x ").collect();
        if parts.len() == 2 {
            let n: usize = parts[0].trim().parse().unwrap_or(0);
            let elem = parse_type_str(parts[1].trim());
            LlvmType::Array(n, Box::new(elem))
        } else {
            LlvmType::Ptr
        }
    } else if s.starts_with('<') && s.ends_with('>') {
        let inner = &s[1..s.len() - 1].trim();
        let parts: Vec<&str> = inner.split(" x ").collect();
        if parts.len() == 2 {
            let n: usize = parts[0].trim().parse().unwrap_or(1);
            let elem = parse_type_str(parts[1].trim());
            LlvmType::Array(n, Box::new(elem))
        } else {
            LlvmType::I64
        }
    } else if s.starts_with('%') {
        LlvmType::Ident(s.to_string())
    } else {
        LlvmType::Ident(s.to_string())
    }
}

fn parse_constant_expr(s: &str) -> Option<LlvmValue> {
    let s = s.trim();
    if s.starts_with("inttoptr (") && s.ends_with(')') {
        let inner = &s[10..s.len() - 1].trim();
        let parts: Vec<&str> = inner.split(" to ").collect();
        if parts.len() == 2 {
            let val_str = parts[0].split_whitespace().last().unwrap_or("0");
            let v = parse_value_str(val_str);
            return Some(LlvmValue::IntToPtr(Box::new(v)));
        }
    }
    if s.starts_with("bitcast (") && s.ends_with(')') {
        let inner = &s[9..s.len() - 1].trim();
        let parts: Vec<&str> = inner.split(" to ").collect();
        if parts.len() == 2 {
            let val_str = parts[0].split_whitespace().last().unwrap_or("0");
            return Some(parse_value_str(val_str));
        }
    }
    if s.starts_with("ptrtoint (") && s.ends_with(')') {
        let inner = &s[10..s.len() - 1].trim();
        let parts: Vec<&str> = inner.split(" to ").collect();
        if parts.len() == 2 {
            let val_str = parts[0].split_whitespace().last().unwrap_or("0");
            return Some(parse_value_str(val_str));
        }
    }
    if s.starts_with('<') && s.ends_with('>') {
        let inner = &s[1..s.len() - 1].trim();
        let items = split_top_level_comma(inner);
        if let Some(first) = items.first() {
            let val_str = first.split_whitespace().last().unwrap_or("0");
            return Some(parse_value_str(val_str));
        }
    }
    if s.starts_with("getelementptr") && s.ends_with(')') {
        if let Some(paren_idx) = s.find('(') {
            let inner = &s[paren_idx + 1..s.len() - 1].trim();
            let parts = split_top_level_comma(inner);
            if parts.len() >= 3 {
                let base_str = parts[1].split_whitespace().last().unwrap_or("@null");
                let offset_str = parts[2].split_whitespace().last().unwrap_or("0");
                let base_val = parse_value_str(base_str);
                let offset_val = parse_value_str(offset_str);
                return Some(LlvmValue::GepExpr {
                    base: Box::new(base_val),
                    offset: Box::new(offset_val),
                });
            }
        }
    }
    None
}

fn parse_value_str(s: &str) -> LlvmValue {
    let s = s.trim().trim_end_matches(',');
    if s.is_empty() || s == "undef" || s == "poison" {
        return LlvmValue::Undef;
    }
    if s == "zeroinitializer" {
        return LlvmValue::Int(0);
    }
    if s == "null" {
        return LlvmValue::Null;
    }
    if s == "true" {
        return LlvmValue::Bool(true);
    }
    if s == "false" {
        return LlvmValue::Bool(false);
    }
    if let Some(c) = parse_constant_expr(s) {
        return c;
    }
    if s.starts_with('%') {
        LlvmValue::Reg(s.to_string())
    } else if s.starts_with('@') {
        LlvmValue::Global(s.to_string())
    } else if s.starts_with("0x") {
        if s.len() == 18 {
            if let Ok(bits) = u64::from_str_radix(&s[2..], 16) {
                return LlvmValue::Float(f64::from_bits(bits));
            }
        } else if s.len() == 10 {
            if let Ok(bits) = u32::from_str_radix(&s[2..], 16) {
                return LlvmValue::Float(f32::from_bits(bits) as f64);
            }
        }
        LlvmValue::Int(i64::from_str_radix(&s[2..], 16).unwrap_or(0))
    } else if let Ok(i) = s.parse::<i64>() {
        LlvmValue::Int(i)
    } else if let Ok(f) = s.parse::<f64>() {
        LlvmValue::Float(f)
    } else {
        LlvmValue::Reg(s.to_string())
    }
}

fn decode_llvm_c_string(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    if out.ends_with('\0') {
        out.pop();
    }
    out
}

fn split_top_level_comma(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    let mut in_str = false;
    let bytes = s.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        match b {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}

// ============================================================================
// Генератор кода Goraw (Lifter)
// ============================================================================

struct GorawLifter<'a> {
    module: &'a Module,
    opts: &'a LlvmToGorawOptions,
    zst_types: HashSet<String>,
}

impl<'a> GorawLifter<'a> {
    fn new(module: &'a Module, opts: &'a LlvmToGorawOptions) -> Self {
        let zst_types = Self::compute_zst_types(module);
        Self {
            module,
            opts,
            zst_types,
        }
    }

    /// Определяет все типы нулевого размера (ZST), такие как PhantomData, Global,
    /// и структуры, все поля которых имеют нулевой размер.
    fn compute_zst_types(module: &Module) -> HashSet<String> {
        let mut zst = HashSet::new();

        // Базовые ZST из стандартной библиотеки Rust
        for s in &module.structs {
            let sname = s.name.to_lowercase();
            if s.fields.is_empty()
                || sname.contains("phantomdata")
                || sname.contains("alloc::alloc::global")
            {
                zst.insert(s.name.clone());
            }
        }

        // Фиксированная точка
        let mut changed = true;
        while changed {
            changed = false;
            for s in &module.structs {
                if !zst.contains(&s.name) && !s.fields.is_empty() {
                    let all_zst = s.fields.iter().all(|f| match f {
                        LlvmType::Void => true,
                        LlvmType::Ident(id) | LlvmType::Struct(id, _) => zst.contains(id),
                        _ => false,
                    });
                    if all_zst {
                        zst.insert(s.name.clone());
                        changed = true;
                    }
                }
            }
        }

        zst
    }

    fn is_zst_type(&self, ty: &LlvmType) -> bool {
        match ty {
            LlvmType::Void => true,
            LlvmType::Ident(id) | LlvmType::Struct(id, _) => self.zst_types.contains(id),
            LlvmType::AnonStruct(fields) => fields.is_empty() || fields.iter().all(|f| self.is_zst_type(f)),
            _ => false,
        }
    }

    fn lift_module(&self) -> String {
        let mut out = String::new();

        if self.opts.emit_comments {
            out.push_str("// Декомпилировано из LLVM IR транслятором Goraw (llvm_to_goraw)\n");
            out.push_str("// Восстановление структурированного CFG, ZST-элиминация, единая область видимости\n\n");
        }

        // 0. Собираем и объявляем анонимные структуры { ... }
        let mut anon_structs = Vec::new();
        for s in &self.module.structs {
            for f in &s.fields {
                f.collect_anon_structs(&mut anon_structs);
            }
        }
        for g in &self.module.globals {
            g.ty.collect_anon_structs(&mut anon_structs);
        }
        for d in &self.module.declares {
            d.ret_ty.collect_anon_structs(&mut anon_structs);
            for (pty, _) in &d.params {
                pty.collect_anon_structs(&mut anon_structs);
            }
        }
        for f in &self.module.functions {
            f.ret_ty.collect_anon_structs(&mut anon_structs);
            for (pty, _) in &f.params {
                pty.collect_anon_structs(&mut anon_structs);
            }
            for b in &f.blocks {
                for inst in &b.instructions {
                    match inst {
                        Instruction::Alloca { ty, .. }
                        | Instruction::Load { ty, .. }
                        | Instruction::Cast { dest_ty: ty, .. }
                        | Instruction::Gep { elem_ty: ty, .. }
                        | Instruction::Phi { ty, .. } => {
                            ty.collect_anon_structs(&mut anon_structs);
                        }
                        Instruction::Call { ret_ty, .. } => {
                            ret_ty.collect_anon_structs(&mut anon_structs);
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut emitted_anons: HashSet<String> = HashSet::new();
        for fields in &anon_structs {
            let non_zst: Vec<(usize, &LlvmType)> = fields
                .iter()
                .enumerate()
                .filter(|(_, f)| !self.is_zst_type(f))
                .collect();
            if non_zst.is_empty() {
                continue;
            }
            let anon_ty = LlvmType::AnonStruct(fields.clone());
            let anon_name = anon_ty.to_goraw_type();
            if emitted_anons.insert(anon_name.clone()) {
                out.push_str(&format!("struct {anon_name} {{\n"));
                for (new_idx, (_, fty)) in non_zst.iter().enumerate() {
                    out.push_str(&format!("    field{new_idx}: {},\n", fty.to_goraw_type()));
                }
                out.push_str("}\n\n");
            }
        }

        // 1. Определение структур (пропуская ZST структуры)
        for s in &self.module.structs {
            if self.zst_types.contains(&s.name) {
                continue;
            }
            let sname = sanitize_type_name(&s.name);
            if sname == "slice" {
                continue;
            }

            // Фильтруем ZST поля внутри структуры
            let non_zst_fields: Vec<(usize, &LlvmType)> = s
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| !self.is_zst_type(f))
                .collect();

            if non_zst_fields.is_empty() {
                continue;
            }

            out.push_str(&format!("struct {sname} {{\n"));
            for (new_idx, (_, fty)) in non_zst_fields.iter().enumerate() {
                out.push_str(&format!("    field{new_idx}: {},\n", fty.to_goraw_type()));
            }
            out.push_str("}\n\n");
        }

        // 2. Глобальные константы и переменные
        for g in &self.module.globals {
            if self.is_zst_type(&g.ty) {
                continue;
            }
            if self.find_string_constant(&g.name).is_some() {
                continue;
            }
            let gname = sanitize_ident(&g.name);
            let gty = g.ty.to_goraw_type();
            let init_val = g
                .init_val
                .as_ref()
                .map(|v| self.format_value(v, &HashMap::new(), &HashMap::new()))
                .unwrap_or_else(|| g.ty.default_zero_val(&self.module.structs));

            let is_agg = matches!(g.ty, LlvmType::Array(..) | LlvmType::Struct(..) | LlvmType::AnonStruct(..));
            if g.is_constant && !is_agg {
                out.push_str(&format!("const {gname}: {gty} = {init_val};\n"));
            } else {
                out.push_str(&format!("static {gname}: {gty} = {init_val};\n"));
            }
        }
        if !self.module.globals.is_empty() {
            out.push('\n');
        }

        // 3. Объявления внешних функций (extern fn ...)
        let uses_memset = self.module.declares.iter().any(|d| d.name.contains("memset"))
            || self.module.functions.iter().any(|f| f.blocks.iter().any(|b| b.instructions.iter().any(|i| match i {
                Instruction::Call { callee, .. } => callee.contains("memset"),
                _ => false,
            })));
        let uses_memcpy = self.module.declares.iter().any(|d| d.name.contains("memcpy"))
            || self.module.functions.iter().any(|f| f.blocks.iter().any(|b| b.instructions.iter().any(|i| match i {
                Instruction::Call { callee, .. } => callee.contains("memcpy"),
                _ => false,
            })));
        if uses_memset {
            out.push_str("extern fn memset(arg0: *mut u8, arg1: i32, arg2: usize) -> *mut u8;\n");
        }
        if uses_memcpy {
            out.push_str("extern fn memcpy(arg0: *mut u8, arg1: *u8, arg2: usize) -> *mut u8;\n");
        }

        for d in &self.module.declares {
            if d.name.starts_with("@llvm.") {
                continue;
            }
            let dname = sanitize_fn_name(&d.name);
            let ret_part = if d.ret_ty == LlvmType::Void {
                String::new()
            } else {
                format!(" -> {}", d.ret_ty.to_goraw_type())
            };
            let mut params_str = Vec::new();
            for (idx, (ty, pname)) in d.params.iter().enumerate() {
                let name = if pname.is_empty() {
                    format!("arg{idx}")
                } else {
                    sanitize_ident(pname)
                };
                params_str.push(format!("{name}: {}", ty.to_goraw_type()));
            }
            if d.is_vararg {
                params_str.push("...".to_string());
            }
            out.push_str(&format!(
                "extern fn {dname}({}){ret_part};\n",
                params_str.join(", ")
            ));
        }
        if !self.module.declares.is_empty() {
            out.push('\n');
        }

        // 4. Определение функций
        for f in &self.module.functions {
            out.push_str(&self.lift_function(f));
            out.push_str("\n\n");
        }

        out
    }

    fn lift_function(&self, f: &FnDef) -> String {
        let mut out = String::new();
        let fname = sanitize_fn_name(&f.name);

        let ret_part = if f.ret_ty == LlvmType::Void {
            String::new()
        } else {
            format!(" -> {}", f.ret_ty.to_goraw_type())
        };

        // 1. Уникальные имена параметров функции (устраняем дубликаты %self.0, %self.1)
        let mut used_names: HashSet<String> = HashSet::new();
        let mut param_names: Vec<String> = Vec::new();
        let mut reg_to_var: HashMap<String, String> = HashMap::new();

        for (idx, (_, orig_name)) in f.params.iter().enumerate() {
            let base = sanitize_var_name(orig_name);
            let mut candidate = if base.is_empty() {
                format!("arg{idx}")
            } else {
                base.clone()
            };
            let mut counter = 1;
            while used_names.contains(&candidate) {
                candidate = format!("{base}_{counter}");
                counter += 1;
            }
            used_names.insert(candidate.clone());
            param_names.push(candidate.clone());

            reg_to_var.insert(orig_name.clone(), candidate.clone());
            reg_to_var.insert(format!("%{idx}"), candidate);
        }

        let params_str: Vec<String> = f
            .params
            .iter()
            .enumerate()
            .map(|(i, (ty, _))| format!("{}: {}", param_names[i], ty.to_goraw_type()))
            .collect();

        // 2. Сбор всех определяемых SSA-регистров и alloca для единого подъема области видимости
        let mut reg_types: HashMap<String, LlvmType> = HashMap::new();
        let mut alloca_vars: HashMap<String, (String, LlvmType)> = HashMap::new();
        let mut does_raw_deref = false;

        for block in &f.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Alloca { res, ty } => {
                        alloca_vars.insert(res.clone(), (sanitize_var_name(res), ty.clone()));
                        reg_types.insert(res.clone(), ty.clone());
                    }
                    Instruction::Load { res, ty, ptr } => {
                        reg_types.insert(res.clone(), ty.clone());
                        if let LlvmValue::Reg(r) = ptr {
                            if !alloca_vars.contains_key(r) {
                                does_raw_deref = true;
                            }
                        }
                    }
                    Instruction::Store { ptr, .. } => {
                        if let LlvmValue::Reg(r) = ptr {
                            if !alloca_vars.contains_key(r) {
                                does_raw_deref = true;
                            }
                        }
                    }
                    Instruction::BinOp { res, ty, .. }
                    | Instruction::Neg { res, ty, .. }
                    | Instruction::Select { res, ty, .. } => {
                        reg_types.insert(res.clone(), ty.clone());
                    }
                    Instruction::ICmp { res, .. } | Instruction::FCmp { res, .. } => {
                        reg_types.insert(res.clone(), LlvmType::I1);
                    }
                    Instruction::Cast { res, dest_ty, .. } => {
                        reg_types.insert(res.clone(), dest_ty.clone());
                    }
                    Instruction::Gep { res, .. } => {
                        reg_types.insert(res.clone(), LlvmType::Ptr);
                    }
                    Instruction::Call { res: Some(r), ret_ty, .. } => {
                        reg_types.insert(r.clone(), ret_ty.clone());
                    }
                    Instruction::Phi { res, ty, .. } => {
                        reg_types.insert(res.clone(), ty.clone());
                    }
                    _ => {}
                }
            }
        }

        // 3. Выделение уникальных неконфликтующих имён для всех SSA-переменных
        for (reg, _) in &reg_types {
            if reg_to_var.contains_key(reg) {
                continue;
            }
            let base = sanitize_var_name(reg);
            let mut candidate = if base.is_empty() {
                "_t".to_string()
            } else {
                base.clone()
            };
            let mut counter = 1;
            while used_names.contains(&candidate) {
                candidate = format!("{base}_{counter}");
                counter += 1;
            }
            used_names.insert(candidate.clone());
            reg_to_var.insert(reg.clone(), candidate);
        }

        let fn_kw = if does_raw_deref { "unsafe fn" } else { "fn" };
        out.push_str(&format!("{fn_kw} {fname}({}){ret_part} {{\n", params_str.join(", ")));

        // 4. ЕДИНАЯ декларация всех локальных переменных в заголовке функции (без дубликатов и затенения!)
        for (reg, ty) in &reg_types {
            if self.is_zst_type(ty) {
                continue;
            }
            if let Some(var_name) = reg_to_var.get(reg) {
                if !param_names.contains(var_name) {
                    let ty_str = ty.to_goraw_type();
                    let zero = ty.default_zero_val(&self.module.structs);
                    out.push_str(&format!("    let mut {var_name}: {ty_str} = {zero};\n"));
                }
            }
        }

        let mut ssa_aliases: HashMap<String, String> = HashMap::new();

        // 5. Структурный лифтинг графа потока управления (CFG)
        if self.opts.structured_cfg && !f.blocks.is_empty() {
            let reduced_blocks = self.reduce_cfg_structured(&f.blocks);
            if reduced_blocks.len() == 1 {
                let b = &reduced_blocks[0];
                self.lift_block_instructions(b, &mut out, &alloca_vars, &reg_to_var, &mut ssa_aliases, "    ");
                self.lift_terminator(&b.terminator, &mut out, &reg_to_var, &ssa_aliases, "    ");
            } else {
                let mut reduced_f = f.clone();
                reduced_f.blocks = reduced_blocks;
                self.lift_cfg_state_machine(&reduced_f, &mut out, &alloca_vars, &reg_to_var, &mut ssa_aliases);
            }
        } else {
            self.lift_cfg_state_machine(f, &mut out, &alloca_vars, &reg_to_var, &mut ssa_aliases);
        }

        let trimmed = out.trim_end();
        let last_line = trimmed.lines().last().unwrap_or("").trim();
        if f.ret_ty != LlvmType::Void && !last_line.starts_with("return") {
            let zero = f.ret_ty.default_zero_val(&self.module.structs);
            out.push_str(&format!("    return {zero};\n"));
        }

        out.push('}');
        out
    }

    // ========================================================================
    // Алгоритм быстрого локального сворачивания CFG (Local Structuring Reductions)
    // ========================================================================

    fn reduce_cfg_structured(&self, initial_blocks: &[BasicBlock]) -> Vec<BasicBlock> {
        let mut blocks = initial_blocks.to_vec();
        let entry_label = blocks[0].label.clone();

        loop {
            let mut changed = false;

            // 1. Линейное слияние (linear block fusion)
            let mut preds: HashMap<String, Vec<String>> = HashMap::new();
            for b in &blocks {
                preds.entry(b.label.clone()).or_default();
                for succ in get_successors(&b.terminator) {
                    preds.entry(succ).or_default().push(b.label.clone());
                }
            }

            let mut to_fuse = None;
            for b in &blocks {
                if let Terminator::Br(target) = &b.terminator {
                    if target != &entry_label && target != &b.label {
                        if let Some(target_preds) = preds.get(target) {
                            if target_preds.len() == 1 && target_preds[0] == b.label {
                                to_fuse = Some((b.label.clone(), target.clone()));
                                break;
                            }
                        }
                    }
                }
            }

            if let Some((src_lbl, dst_lbl)) = to_fuse {
                let dst_block = blocks.iter().find(|b| b.label == dst_lbl).cloned();
                if let Some(dst) = dst_block {
                    if let Some(src) = blocks.iter_mut().find(|b| b.label == src_lbl) {
                        src.instructions.extend(dst.instructions);
                        src.terminator = dst.terminator;
                        changed = true;
                    }
                    blocks.retain(|b| b.label != dst_lbl);
                }
                if changed {
                    continue;
                }
            }

            // 2. Сворачивание условных ветвей (IfThen, IfElse, IfThenRet, While)
            enum Reduction {
                IfThen {
                    cond: LlvmValue,
                    then_lbl: String,
                    next_lbl: String,
                },
                IfThenRet {
                    cond: LlvmValue,
                    then_lbl: String,
                    next_lbl: String,
                    ret_val: Option<LlvmValue>,
                },
                IfElse {
                    cond: LlvmValue,
                    then_lbl: String,
                    else_lbl: String,
                    join_lbl: String,
                },
                While {
                    cond: LlvmValue,
                    body_lbl: String,
                    exit_lbl: String,
                },
            }

            let mut if_reduction = None;
            for (idx, b) in blocks.iter().enumerate() {
                if let Terminator::CondBr {
                    cond,
                    then_label,
                    else_label,
                } = &b.terminator
                {
                    // Проверка IfThen / IfThenRet
                    let then_single = then_label != &entry_label
                        && preds
                            .get(then_label)
                            .map(|p| p.len() == 1 && p[0] == b.label)
                            .unwrap_or(false);

                    if then_single {
                        if let Some(then_b) = blocks.iter().find(|bl| &bl.label == then_label) {
                            if let Terminator::Br(target) = &then_b.terminator {
                                if target == else_label {
                                    if_reduction = Some((
                                        idx,
                                        Reduction::IfThen {
                                            cond: cond.clone(),
                                            then_lbl: then_label.clone(),
                                            next_lbl: else_label.clone(),
                                        },
                                    ));
                                    break;
                                } else if target == &b.label {
                                    // While цикл
                                    if_reduction = Some((
                                        idx,
                                        Reduction::While {
                                            cond: cond.clone(),
                                            body_lbl: then_label.clone(),
                                            exit_lbl: else_label.clone(),
                                        },
                                    ));
                                    break;
                                }
                            } else if let Terminator::Ret(ret_val) = &then_b.terminator {
                                if_reduction = Some((
                                    idx,
                                    Reduction::IfThenRet {
                                        cond: cond.clone(),
                                        then_lbl: then_label.clone(),
                                        next_lbl: else_label.clone(),
                                        ret_val: ret_val.clone(),
                                    },
                                ));
                                break;
                            }
                        }
                    }

                    // Проверка обратного IfThen / IfThenRet
                    let else_single = else_label != &entry_label
                        && preds
                            .get(else_label)
                            .map(|p| p.len() == 1 && p[0] == b.label)
                            .unwrap_or(false);

                    if else_single {
                        if let Some(else_b) = blocks.iter().find(|bl| &bl.label == else_label) {
                            if let Terminator::Br(target) = &else_b.terminator {
                                if target == then_label {
                                    let inv_cond = invert_condition(cond);
                                    if_reduction = Some((
                                        idx,
                                        Reduction::IfThen {
                                            cond: inv_cond,
                                            then_lbl: else_label.clone(),
                                            next_lbl: then_label.clone(),
                                        },
                                    ));
                                    break;
                                }
                            } else if let Terminator::Ret(ret_val) = &else_b.terminator {
                                let inv_cond = invert_condition(cond);
                                if_reduction = Some((
                                    idx,
                                    Reduction::IfThenRet {
                                        cond: inv_cond,
                                        then_lbl: else_label.clone(),
                                        next_lbl: then_label.clone(),
                                        ret_val: ret_val.clone(),
                                    },
                                ));
                                break;
                            }
                        }
                    }

                    // Проверка IfElse: then и else оба имеют 1 pred и прыгают на общий join
                    if then_single && else_single {
                        let then_b = blocks.iter().find(|bl| &bl.label == then_label);
                        let else_b = blocks.iter().find(|bl| &bl.label == else_label);
                        if let (Some(tb), Some(eb)) = (then_b, else_b) {
                            if let (Terminator::Br(t_target), Terminator::Br(e_target)) =
                                (&tb.terminator, &eb.terminator)
                            {
                                if t_target == e_target {
                                    if_reduction = Some((
                                        idx,
                                        Reduction::IfElse {
                                            cond: cond.clone(),
                                            then_lbl: then_label.clone(),
                                            else_lbl: else_label.clone(),
                                            join_lbl: t_target.clone(),
                                        },
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            if let Some((src_idx, red)) = if_reduction {
                match red {
                    Reduction::IfThen {
                        cond,
                        then_lbl,
                        next_lbl,
                    } => {
                        let then_insts = blocks
                            .iter()
                            .find(|b| b.label == then_lbl)
                            .map(|b| b.instructions.clone())
                            .unwrap_or_default();
                        blocks[src_idx].instructions.push(Instruction::IfThen { cond, then_insts });
                        blocks[src_idx].terminator = Terminator::Br(next_lbl);
                        blocks.retain(|b| b.label != then_lbl);
                        changed = true;
                    }
                    Reduction::IfThenRet {
                        cond,
                        then_lbl,
                        next_lbl,
                        ret_val,
                    } => {
                        let then_insts = blocks
                            .iter()
                            .find(|b| b.label == then_lbl)
                            .map(|b| b.instructions.clone())
                            .unwrap_or_default();
                        blocks[src_idx].instructions.push(Instruction::IfThenRet {
                            cond,
                            then_insts,
                            ret_val,
                        });
                        blocks[src_idx].terminator = Terminator::Br(next_lbl);
                        blocks.retain(|b| b.label != then_lbl);
                        changed = true;
                    }
                    Reduction::IfElse {
                        cond,
                        then_lbl,
                        else_lbl,
                        join_lbl,
                    } => {
                        let then_insts = blocks
                            .iter()
                            .find(|b| b.label == then_lbl)
                            .map(|b| b.instructions.clone())
                            .unwrap_or_default();
                        let else_insts = blocks
                            .iter()
                            .find(|b| b.label == else_lbl)
                            .map(|b| b.instructions.clone())
                            .unwrap_or_default();
                        blocks[src_idx].instructions.push(Instruction::IfElse {
                            cond,
                            then_insts,
                            else_insts,
                        });
                        blocks[src_idx].terminator = Terminator::Br(join_lbl);
                        blocks.retain(|b| b.label != then_lbl && b.label != else_lbl);
                        changed = true;
                    }
                    Reduction::While {
                        cond,
                        body_lbl,
                        exit_lbl,
                    } => {
                        let body_insts = blocks
                            .iter()
                            .find(|b| b.label == body_lbl)
                            .map(|b| b.instructions.clone())
                            .unwrap_or_default();
                        blocks[src_idx].instructions.push(Instruction::While { cond, body_insts });
                        blocks[src_idx].terminator = Terminator::Br(exit_lbl);
                        blocks.retain(|b| b.label != body_lbl);
                        changed = true;
                    }
                }
            }

            if !changed {
                break;
            }
        }

        blocks
    }

    fn lift_cfg_state_machine(
        &self,
        f: &FnDef,
        out: &mut String,
        alloca_vars: &HashMap<String, (String, LlvmType)>,
        reg_to_var: &HashMap<String, String>,
        ssa_aliases: &mut HashMap<String, String>,
    ) {
        let mut label_to_id: HashMap<String, usize> = HashMap::new();
        for (i, b) in f.blocks.iter().enumerate() {
            label_to_id.insert(b.label.clone(), i);
        }

        out.push_str("    let mut __bb: i32 = 0;\n");
        out.push_str("    while true {\n");
        out.push_str("        match __bb {\n");

        for (i, b) in f.blocks.iter().enumerate() {
            out.push_str(&format!("            {i} => {{\n"));
            self.lift_block_instructions(b, out, alloca_vars, reg_to_var, ssa_aliases, "                ");

            match &b.terminator {
                Terminator::Br(target) => {
                    self.emit_phi_assignments(f, out, target, &b.label, reg_to_var, ssa_aliases, "                ");
                    let tid = label_to_id.get(target).cloned().unwrap_or(0);
                    out.push_str(&format!("                __bb = {tid};\n"));
                }
                Terminator::CondBr {
                    cond,
                    then_label,
                    else_label,
                } => {
                    let cond_s = self.format_value(cond, reg_to_var, ssa_aliases);
                    let then_id = label_to_id.get(then_label).cloned().unwrap_or(0);
                    let else_id = label_to_id.get(else_label).cloned().unwrap_or(0);
                    out.push_str(&format!("                if {cond_s} {{\n"));
                    self.emit_phi_assignments(f, out, then_label, &b.label, reg_to_var, ssa_aliases, "                    ");
                    out.push_str(&format!("                    __bb = {then_id};\n"));
                    out.push_str("                } else {\n");
                    self.emit_phi_assignments(f, out, else_label, &b.label, reg_to_var, ssa_aliases, "                    ");
                    out.push_str(&format!("                    __bb = {else_id};\n"));
                    out.push_str("                }\n");
                }
                Terminator::Switch {
                    val,
                    default_label,
                    cases,
                } => {
                    let val_s = self.format_value(val, reg_to_var, ssa_aliases);
                    let def_id = label_to_id.get(default_label).cloned().unwrap_or(0);
                    out.push_str(&format!("                match {val_s} {{\n"));
                    for (cval, clabel) in cases {
                        let cs = self.format_value(cval, reg_to_var, ssa_aliases);
                        let cid = label_to_id.get(clabel).cloned().unwrap_or(0);
                        out.push_str(&format!("                    {cs} => {{\n"));
                        self.emit_phi_assignments(f, out, clabel, &b.label, reg_to_var, ssa_aliases, "                        ");
                        out.push_str(&format!("                        __bb = {cid};\n                    }}\n"));
                    }
                    out.push_str(&format!("                    _ => {{\n"));
                    self.emit_phi_assignments(f, out, default_label, &b.label, reg_to_var, ssa_aliases, "                        ");
                    out.push_str(&format!("                        __bb = {def_id};\n                    }}\n"));
                    out.push_str("                }\n");
                }
                Terminator::Ret(Some(val)) => {
                    let vs = self.format_value(val, reg_to_var, ssa_aliases);
                    out.push_str(&format!("                return {vs};\n"));
                }
                Terminator::Ret(None) => {
                    out.push_str("                return;\n");
                }
                Terminator::Unreachable => {
                    out.push_str("                break;\n");
                }
            }

            out.push_str("            }\n");
        }

        out.push_str("            _ => {\n");
        out.push_str("                break;\n");
        out.push_str("            }\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
    }

    fn emit_phi_assignments(
        &self,
        f: &FnDef,
        out: &mut String,
        target_label: &str,
        cur_label: &str,
        reg_to_var: &HashMap<String, String>,
        ssa_aliases: &HashMap<String, String>,
        indent: &str,
    ) {
        if let Some(target_blk) = f.blocks.iter().find(|b| b.label == target_label) {
            for inst in &target_blk.instructions {
                if let Instruction::Phi { res, incoming, .. } = inst {
                    for (val, pred) in incoming {
                        if pred == cur_label {
                            let phi_var_name = reg_to_var
                                .get(res)
                                .cloned()
                                .unwrap_or_else(|| sanitize_var_name(res));
                            let val_s = self.format_value(val, reg_to_var, ssa_aliases);
                            out.push_str(&format!("{indent}{phi_var_name} = {val_s};\n"));
                        }
                    }
                }
            }
        }
    }

    fn lift_block_instructions(
        &self,
        block: &BasicBlock,
        out: &mut String,
        alloca_vars: &HashMap<String, (String, LlvmType)>,
        reg_to_var: &HashMap<String, String>,
        ssa_aliases: &mut HashMap<String, String>,
        indent: &str,
    ) {
        for inst in &block.instructions {
            match inst {
                Instruction::Nop => {}
                Instruction::Alloca { .. } | Instruction::Phi { .. } => {
                    // Уже объявлены в начале функции
                }
                Instruction::Store { val, ptr } => {
                    let ptr_reg = match ptr {
                        LlvmValue::Reg(r) => r.clone(),
                        _ => String::new(),
                    };
                    let val_s = if let LlvmValue::Reg(r) = val {
                        if let Some((vname, _)) = alloca_vars.get(r) {
                            format!("&mut {vname}")
                        } else {
                            self.format_value(val, reg_to_var, ssa_aliases)
                        }
                    } else {
                        self.format_value(val, reg_to_var, ssa_aliases)
                    };

                    if let Some((var_name, _)) = alloca_vars.get(&ptr_reg) {
                        if var_name != &val_s {
                            out.push_str(&format!("{indent}{var_name} = {val_s};\n"));
                        }
                    } else if let Some(alias) = ssa_aliases.get(&ptr_reg) {
                        if alias.contains(".field") || alias.contains('[') {
                            out.push_str(&format!("{indent}{alias} = {val_s};\n"));
                        } else {
                            out.push_str(&format!("{indent}*{alias} = {val_s};\n"));
                        }
                    } else {
                        let ptr_s = self.format_value(ptr, reg_to_var, ssa_aliases);
                        out.push_str(&format!("{indent}*{ptr_s} = {val_s};\n"));
                    }
                }
                Instruction::Load { res, ptr, ty } => {
                    if self.is_zst_type(ty) {
                        continue;
                    }
                    let ptr_reg = match ptr {
                        LlvmValue::Reg(r) => r.clone(),
                        _ => String::new(),
                    };

                    if let Some((var_name, _)) = alloca_vars.get(&ptr_reg) {
                        ssa_aliases.insert(res.clone(), var_name.clone());
                    } else if let Some(alias) = ssa_aliases.get(&ptr_reg) {
                        if alias.contains(".field") || alias.contains('[') {
                            ssa_aliases.insert(res.clone(), alias.clone());
                        } else {
                            let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                            out.push_str(&format!("{indent}{var_name} = *{alias};\n"));
                            ssa_aliases.insert(res.clone(), var_name);
                        }
                    } else {
                        let ptr_s = self.format_value(ptr, reg_to_var, ssa_aliases);
                        let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                        out.push_str(&format!("{indent}{var_name} = *{ptr_s};\n"));
                        ssa_aliases.insert(res.clone(), var_name);
                    }
                }
                Instruction::BinOp { res, op, lhs, rhs, .. } => {
                    let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                    let lhs_s = self.format_value(lhs, reg_to_var, ssa_aliases);
                    let rhs_s = self.format_value(rhs, reg_to_var, ssa_aliases);
                    let op_sym = match op.as_str() {
                        "add" | "fadd" => "+",
                        "sub" | "fsub" => "-",
                        "mul" | "fmul" => "*",
                        "sdiv" | "udiv" | "fdiv" => "/",
                        "srem" | "urem" => "%",
                        "shl" => "<<",
                        "lshr" | "ashr" => ">>",
                        "and" => "&",
                        "or" => "|",
                        "xor" => "^",
                        _ => "+",
                    };
                    out.push_str(&format!("{indent}{var_name} = {lhs_s} {op_sym} {rhs_s};\n"));
                    ssa_aliases.insert(res.clone(), var_name);
                }
                Instruction::Neg { res, val, .. } => {
                    let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                    let val_s = self.format_value(val, reg_to_var, ssa_aliases);
                    out.push_str(&format!("{indent}{var_name} = -{val_s};\n"));
                    ssa_aliases.insert(res.clone(), var_name);
                }
                Instruction::ICmp { res, cond, lhs, rhs } => {
                    let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                    let lhs_s = self.format_value(lhs, reg_to_var, ssa_aliases);
                    let rhs_s = self.format_value(rhs, reg_to_var, ssa_aliases);
                    let op_sym = match cond.as_str() {
                        "eq" => "==",
                        "ne" => "!=",
                        "slt" | "ult" => "<",
                        "sle" | "ule" => "<=",
                        "sgt" | "ugt" => ">",
                        "sge" | "uge" => ">=",
                        _ => "==",
                    };
                    out.push_str(&format!("{indent}{var_name} = {lhs_s} {op_sym} {rhs_s};\n"));
                    ssa_aliases.insert(res.clone(), var_name);
                }
                Instruction::FCmp { res, cond, lhs, rhs } => {
                    let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                    let lhs_s = self.format_value(lhs, reg_to_var, ssa_aliases);
                    let rhs_s = self.format_value(rhs, reg_to_var, ssa_aliases);
                    let op_sym = match cond.as_str() {
                        "oeq" | "ueq" => "==",
                        "one" | "une" => "!=",
                        "olt" | "ult" => "<",
                        "ole" | "ule" => "<=",
                        "ogt" | "ugt" => ">",
                        "oge" | "uge" => ">=",
                        _ => "==",
                    };
                    out.push_str(&format!("{indent}{var_name} = {lhs_s} {op_sym} {rhs_s};\n"));
                    ssa_aliases.insert(res.clone(), var_name);
                }
                Instruction::Cast { res, dest_ty, val } => {
                    if self.is_zst_type(dest_ty) {
                        continue;
                    }
                    let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                    let val_s = self.format_value(val, reg_to_var, ssa_aliases);
                    let dty = dest_ty.to_goraw_type();
                    out.push_str(&format!("{indent}{var_name} = {val_s} as {dty};\n"));
                    ssa_aliases.insert(res.clone(), var_name);
                }
                Instruction::Gep {
                    res,
                    elem_ty,
                    ptr,
                    indices,
                } => {
                    let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                    let ptr_reg = match ptr {
                        LlvmValue::Reg(r) => r.clone(),
                        _ => String::new(),
                    };
                    let base_name = if let Some((vname, _)) = alloca_vars.get(&ptr_reg) {
                        vname.clone()
                    } else if let Some(alias) = ssa_aliases.get(&ptr_reg) {
                        alias.clone()
                    } else {
                        self.format_value(ptr, reg_to_var, ssa_aliases)
                    };

                    if indices.len() >= 2 {
                        if let LlvmValue::Int(field_idx) = indices[1] {
                            let field_expr = format!("{base_name}.field{field_idx}");
                            ssa_aliases.insert(res.clone(), field_expr);
                            continue;
                        }
                    } else if indices.len() == 1 {
                        let idx_s = self.format_value(&indices[0], reg_to_var, ssa_aliases);
                        let elem_expr = format!("{base_name}[{idx_s}]");
                        ssa_aliases.insert(res.clone(), elem_expr);
                        continue;
                    }

                    let mut offset_expr = String::new();
                    for idx in indices {
                        let idx_s = self.format_value(idx, reg_to_var, ssa_aliases);
                        if offset_expr.is_empty() {
                            offset_expr = idx_s;
                        } else {
                            offset_expr = format!("{offset_expr} + {idx_s}");
                        }
                    }
                    let ety = elem_ty.to_goraw_type();
                    out.push_str(&format!(
                        "{indent}{var_name} = ({base_name} as *mut {ety}) + ({offset_expr});\n"
                    ));
                    ssa_aliases.insert(res.clone(), var_name);
                }
                Instruction::Call {
                    res,
                    callee,
                    args,
                    ..
                } => {
                    // Игнорируем оптимизационные интринсики LLVM
                    if callee.starts_with("@llvm.lifetime.")
                        || callee.starts_with("@llvm.assume")
                        || callee.starts_with("@llvm.experimental.")
                        || callee.starts_with("@llvm.dbg.")
                    {
                        continue;
                    }

                    let mut args_s = Vec::new();
                    for a in args {
                        if let LlvmValue::Global(g) = a {
                            if let Some(slit) = self.module.string_constants.get(g) {
                                args_s.push(format!("\"{}\"", escape_goraw_str(slit)));
                                continue;
                            }
                        }
                        args_s.push(self.format_value(a, reg_to_var, ssa_aliases));
                    }

                    // Трансляция интринзиков LLVM в builtins Goraw / libc
                    let call_expr = if callee.starts_with("@llvm.sqrt.") {
                        format!("sqrt({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.fabs.") {
                        format!("abs({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.pow.") {
                        format!("pow({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.ceil.") {
                        format!("ceil({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.floor.") {
                        format!("floor({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.smax.") || callee.starts_with("@llvm.umax.") {
                        format!("max({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.smin.") || callee.starts_with("@llvm.umin.") {
                        format!("min({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.memcpy.") {
                        if args_s.len() >= 3 {
                            format!("memcpy({} as *mut u8, {} as *u8, {} as usize)", args_s[0], args_s[1], args_s[2])
                        } else {
                            format!("memcpy({})", args_s.join(", "))
                        }
                    } else if callee.starts_with("@llvm.memmove.") {
                        if args_s.len() >= 3 {
                            format!("memmove({} as *mut u8, {} as *u8, {} as usize)", args_s[0], args_s[1], args_s[2])
                        } else {
                            format!("memmove({})", args_s.join(", "))
                        }
                    } else if callee.starts_with("@llvm.memset.") {
                        if args_s.len() >= 3 {
                            format!("memset({} as *mut u8, {} as i32, {} as usize)", args_s[0], args_s[1], args_s[2])
                        } else {
                            format!("memset({})", args_s.join(", "))
                        }
                    } else if callee.starts_with("@llvm.bswap.i16") {
                        format!("((({} >> 8) & 0xff) | (({} << 8) & 0xff00))", args_s[0], args_s[0])
                    } else if callee.starts_with("@llvm.bswap.") {
                        format!("bswap({})", args_s.join(", "))
                    } else if callee.starts_with("@llvm.cttz.") {
                        format!("cttz({})", args_s[0])
                    } else if callee.starts_with("@llvm.ctlz.") {
                        format!("ctlz({})", args_s[0])
                    } else if callee.starts_with("@llvm.ctpop.") {
                        format!("ctpop({})", args_s[0])
                    } else if callee.starts_with("@llvm.expect.") {
                        args_s.first().cloned().unwrap_or_else(|| "0".to_string())
                    } else {
                        let fn_name = sanitize_fn_name(callee);
                        format!("{fn_name}({})", args_s.join(", "))
                    };

                    if let Some(r) = res {
                        let var_name = reg_to_var.get(r).cloned().unwrap_or_else(|| sanitize_var_name(r));
                        out.push_str(&format!("{indent}{var_name} = {call_expr};\n"));
                        ssa_aliases.insert(r.clone(), var_name);
                    } else {
                        out.push_str(&format!("{indent}{call_expr};\n"));
                    }
                }
                Instruction::Select {
                    res,
                    cond,
                    val_true,
                    val_false,
                    ..
                } => {
                    let var_name = reg_to_var.get(res).cloned().unwrap_or_else(|| sanitize_var_name(res));
                    let cond_s = self.format_value(cond, reg_to_var, ssa_aliases);
                    let vt_s = self.format_value(val_true, reg_to_var, ssa_aliases);
                    let vf_s = self.format_value(val_false, reg_to_var, ssa_aliases);
                    out.push_str(&format!(
                        "{indent}{var_name} = if {cond_s} {{ {vt_s} }} else {{ {vf_s} }};\n"
                    ));
                    ssa_aliases.insert(res.clone(), var_name);
                }
                Instruction::IfThen { cond, then_insts } => {
                    let cond_s = self.format_value(cond, reg_to_var, ssa_aliases);
                    out.push_str(&format!("{indent}if {cond_s} {{\n"));
                    let inner_indent = format!("{indent}    ");
                    let fake_block = BasicBlock {
                        label: String::new(),
                        instructions: then_insts.clone(),
                        terminator: Terminator::Ret(None),
                    };
                    self.lift_block_instructions(&fake_block, out, alloca_vars, reg_to_var, ssa_aliases, &inner_indent);
                    out.push_str(&format!("{indent}}}\n"));
                }
                Instruction::IfElse { cond, then_insts, else_insts } => {
                    let cond_s = self.format_value(cond, reg_to_var, ssa_aliases);
                    out.push_str(&format!("{indent}if {cond_s} {{\n"));
                    let inner_indent = format!("{indent}    ");
                    let fake_then = BasicBlock {
                        label: String::new(),
                        instructions: then_insts.clone(),
                        terminator: Terminator::Ret(None),
                    };
                    self.lift_block_instructions(&fake_then, out, alloca_vars, reg_to_var, ssa_aliases, &inner_indent);
                    out.push_str(&format!("{indent}}} else {{\n"));
                    let fake_else = BasicBlock {
                        label: String::new(),
                        instructions: else_insts.clone(),
                        terminator: Terminator::Ret(None),
                    };
                    self.lift_block_instructions(&fake_else, out, alloca_vars, reg_to_var, ssa_aliases, &inner_indent);
                    out.push_str(&format!("{indent}}}\n"));
                }
                Instruction::IfThenRet { cond, then_insts, ret_val } => {
                    let cond_s = self.format_value(cond, reg_to_var, ssa_aliases);
                    out.push_str(&format!("{indent}if {cond_s} {{\n"));
                    let inner_indent = format!("{indent}    ");
                    let fake_then = BasicBlock {
                        label: String::new(),
                        instructions: then_insts.clone(),
                        terminator: Terminator::Ret(ret_val.clone()),
                    };
                    self.lift_block_instructions(&fake_then, out, alloca_vars, reg_to_var, ssa_aliases, &inner_indent);
                    self.lift_terminator(&Terminator::Ret(ret_val.clone()), out, reg_to_var, ssa_aliases, &inner_indent);
                    out.push_str(&format!("{indent}}}\n"));
                }
                Instruction::While { cond, body_insts } => {
                    let cond_s = self.format_value(cond, reg_to_var, ssa_aliases);
                    out.push_str(&format!("{indent}while {cond_s} {{\n"));
                    let inner_indent = format!("{indent}    ");
                    let fake_body = BasicBlock {
                        label: String::new(),
                        instructions: body_insts.clone(),
                        terminator: Terminator::Ret(None),
                    };
                    self.lift_block_instructions(&fake_body, out, alloca_vars, reg_to_var, ssa_aliases, &inner_indent);
                    out.push_str(&format!("{indent}}}\n"));
                }
            }
        }
    }

    fn lift_terminator(
        &self,
        term: &Terminator,
        out: &mut String,
        reg_to_var: &HashMap<String, String>,
        ssa_aliases: &HashMap<String, String>,
        indent: &str,
    ) {
        match term {
            Terminator::Ret(Some(val)) => {
                let vs = self.format_value(val, reg_to_var, ssa_aliases);
                out.push_str(&format!("{indent}return {vs};\n"));
            }
            Terminator::Ret(None) => {
                out.push_str(&format!("{indent}return;\n"));
            }
            Terminator::Br(_) => {}
            Terminator::CondBr { .. } => {}
            Terminator::Switch { .. } => {}
            Terminator::Unreachable => {
                out.push_str(&format!("{indent}panic(\"unreachable\");\n"));
            }
        }
    }

    fn find_string_constant(&self, g: &str) -> Option<&String> {
        let clean = g.trim_matches('"');
        let unquoted = if clean.starts_with('@') { &clean[1..] } else { clean };
        if let Some(s) = self.module.string_constants.get(g) {
            return Some(s);
        }
        if let Some(s) = self.module.string_constants.get(clean) {
            return Some(s);
        }
        if let Some(s) = self.module.string_constants.get(unquoted) {
            return Some(s);
        }
        for (k, v) in &self.module.string_constants {
            let k_clean = k.trim_matches('"');
            let k_unquoted = if k_clean.starts_with('@') { &k_clean[1..] } else { k_clean };
            if k_unquoted == unquoted || k == g || k == clean {
                return Some(v);
            }
        }
        None
    }

    fn format_value(
        &self,
        val: &LlvmValue,
        reg_to_var: &HashMap<String, String>,
        ssa_aliases: &HashMap<String, String>,
    ) -> String {
        match val {
            LlvmValue::Reg(r) => {
                if let Some(alias) = ssa_aliases.get(r) {
                    alias.clone()
                } else if let Some(vname) = reg_to_var.get(r) {
                    vname.clone()
                } else {
                    sanitize_var_name(r)
                }
            }
            LlvmValue::Global(g) => {
                if let Some(slit) = self.find_string_constant(g) {
                    format!("\"{}\"", escape_goraw_str(slit))
                } else {
                    sanitize_ident(g)
                }
            }
            LlvmValue::Int(i) => i.to_string(),
            LlvmValue::Float(f) => {
                let s = f.to_string();
                if s.contains('.') {
                    s
                } else {
                    format!("{s}.0")
                }
            }
            LlvmValue::Bool(b) => b.to_string(),
            LlvmValue::Null => "null".to_string(),
            LlvmValue::Undef => "0".to_string(),
            LlvmValue::StringLit(s) => format!("\"{}\"", escape_goraw_str(s)),
            LlvmValue::IntToPtr(inner) => {
                let inner_s = self.format_value(inner, reg_to_var, ssa_aliases);
                format!("({inner_s} as *mut u8)")
            }
            LlvmValue::GepExpr { base, offset } => {
                let base_s = self.format_value(base, reg_to_var, ssa_aliases);
                let offset_s = self.format_value(offset, reg_to_var, ssa_aliases);
                format!("(({base_s} as *mut u8).add({offset_s}))")
            }
        }
    }
}

fn invert_condition(cond: &LlvmValue) -> LlvmValue {
    match cond {
        LlvmValue::Bool(b) => LlvmValue::Bool(!b),
        LlvmValue::Reg(r) => {
            if r.starts_with("!(") && r.ends_with(')') {
                LlvmValue::Reg(r[2..r.len() - 1].to_string())
            } else {
                LlvmValue::Reg(format!("!({r})"))
            }
        }
        _ => LlvmValue::Reg(format!("!({cond:?})")),
    }
}

fn get_successors(term: &Terminator) -> Vec<String> {
    match term {
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
        Terminator::Br(target) => vec![target.clone()],
        Terminator::CondBr {
            then_label,
            else_label,
            ..
        } => {
            if then_label == else_label {
                vec![then_label.clone()]
            } else {
                vec![then_label.clone(), else_label.clone()]
            }
        }
        Terminator::Switch {
            default_label,
            cases,
            ..
        } => {
            let mut s = vec![default_label.clone()];
            for (_, l) in cases {
                if !s.contains(l) {
                    s.push(l.clone());
                }
            }
            s
        }
    }
}

// ============================================================================
// Вспомогательные функции санации имён и экранирования
// ============================================================================

fn sanitize_ident(name: &str) -> String {
    let clean = name.trim_start_matches('%').trim_start_matches('@');
    let mut out = String::new();
    for ch in clean.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.chars().next().unwrap().is_numeric() {
        out = format!("_t{out}");
    }
    let collapsed = collapse_underscores(&out);
    if collapsed == "_" || collapsed.is_empty() {
        return "_anon".to_string();
    }
    avoid_keywords(&collapsed)
}

fn sanitize_var_name(name: &str) -> String {
    let clean = name.trim_start_matches('%');
    let clean = clean.trim_start_matches("arg.");
    let mut parts: Vec<&str> = clean.split('.').collect();
    if parts.len() > 1 && parts.last().unwrap().chars().all(|c| c.is_numeric()) {
        parts.pop();
    }
    let base = parts.join("_");
    sanitize_ident(&base)
}

fn sanitize_fn_name(name: &str) -> String {
    let clean = name.trim_start_matches('@');
    sanitize_ident(clean)
}

fn sanitize_type_name(name: &str) -> String {
    let clean = name
        .trim_start_matches('%')
        .trim_start_matches("struct.")
        .trim_start_matches("class.")
        .trim_matches('{')
        .trim_matches('}')
        .trim_matches('"')
        .trim();
    sanitize_ident(clean)
}

fn collapse_underscores(s: &str) -> String {
    let mut out = String::new();
    let mut prev_under = false;
    for ch in s.chars() {
        if ch == '_' {
            if !prev_under {
                out.push('_');
                prev_under = true;
            }
        } else {
            out.push(ch);
            prev_under = false;
        }
    }
    out
}

fn avoid_keywords(name: &str) -> String {
    match name {
        "fn" | "let" | "mut" | "if" | "else" | "while" | "for" | "match" | "return" | "struct"
        | "enum" | "type" | "const" | "static" | "unsafe" | "extern" | "true" | "false" | "null"
        | "break" | "continue" | "asm" | "jit" | "self" | "super" | "crate" => format!("{name}_var"),
        _ => name.to_string(),
    }
}

fn escape_goraw_str(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\0' => out.push_str("\\0"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if (c as u32) < 32 || (c as u32) == 127 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Тесты модуля
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_types_and_values() {
        assert_eq!(parse_type_str("i32"), LlvmType::I32);
        assert_eq!(parse_type_str("double"), LlvmType::Double);
        assert_eq!(parse_type_str("ptr"), LlvmType::Ptr);
        assert_eq!(
            parse_type_str("[10 x i32]"),
            LlvmType::Array(10, Box::new(LlvmType::I32))
        );

        assert_eq!(parse_value_str("42"), LlvmValue::Int(42));
        assert_eq!(parse_value_str("true"), LlvmValue::Bool(true));
        assert_eq!(parse_value_str("%x.1"), LlvmValue::Reg("%x.1".to_string()));

        // Hex float: 0x4008000000000000 == 3.0
        assert_eq!(parse_value_str("0x4008000000000000"), LlvmValue::Float(3.0));

        // Константные выражения inttoptr
        assert_eq!(
            parse_value_str("inttoptr (i64 1 to ptr)"),
            LlvmValue::IntToPtr(Box::new(LlvmValue::Int(1)))
        );

        // zeroinitializer
        assert_eq!(parse_value_str("zeroinitializer"), LlvmValue::Int(0));
    }

    #[test]
    fn test_transpile_simple_function() {
        let ir = r#"
define i32 @add(i32 %a, i32 %b) {
entry:
  %t1 = add i32 %a, %b
  ret i32 %t1
}
"#;
        let res = transpile_llvm_ir(ir, &LlvmToGorawOptions::default()).unwrap();
        assert!(res.contains("fn add(a: i32, b: i32) -> i32"));
        assert!(res.contains("return t1;"));
    }

    #[test]
    fn test_transpile_math_roundtrip() {
        let ir = r#"
declare i32 @printf(ptr, ...)
declare double @llvm.sqrt.f64(double)

@.str.0 = private unnamed_addr constant [14 x i8] c"hypot = %f\0A\00", align 1

define double @hypot(double %arg.a, double %arg.b) {
entry:
  %t1 = fmul double %arg.a, %arg.a
  %t2 = fmul double %arg.b, %arg.b
  %t3 = fadd double %t1, %t2
  %t4 = call double @llvm.sqrt.f64(double %t3)
  ret double %t4
}
"#;
        let res = transpile_llvm_ir(ir, &LlvmToGorawOptions::default()).unwrap();
        assert!(res.contains("extern fn printf(arg0: *u8, ...) -> i32;"));
        assert!(res.contains("fn hypot(a: f64, b: f64) -> f64"));
        assert!(res.contains("sqrt("));
    }

    #[test]
    fn test_transpile_loop_and_cfg_structuring() {
        let ir = r#"
define i32 @sum_to_n(i32 %n) {
entry:
  br label %loop_header

loop_header:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop_body ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %loop_body ]
  %cond = icmp slt i32 %i, %n
  br i1 %cond, label %loop_body, label %loop_exit

loop_body:
  %sum.next = add i32 %sum, %i
  %i.next = add i32 %i, 1
  br label %loop_header

loop_exit:
  ret i32 %sum
}
"#;
        let res = transpile_llvm_ir(ir, &LlvmToGorawOptions::default()).unwrap();
        // Убеждаемся, что CFG структурирован как while, а не макаронный __bb
        assert!(res.contains("while cond {") || res.contains("while true {"));
        assert!(!res.contains("match __bb"));
    }

    #[test]
    fn test_zst_elimination_and_intrinsics() {
        let ir = r#"
%"alloc::alloc::Global" = type {}
%"core::marker::PhantomData<u8>" = type {}
%struct.RealData = type { i64, %"core::marker::PhantomData<u8>", ptr }

declare void @llvm.lifetime.start.p0(i64, ptr)
declare void @llvm.assume(i1)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)

define void @process(ptr %p) {
entry:
  call void @llvm.lifetime.start.p0(i64 8, ptr %p)
  call void @llvm.assume(i1 true)
  call void @llvm.memcpy.p0.p0.i64(ptr %p, ptr %p, i64 16, i1 false)
  ret void
}
"#;
        let res = transpile_llvm_ir(ir, &LlvmToGorawOptions::default()).unwrap();
        // ZST структуры должны быть полностью убраны
        assert!(!res.contains("struct _alloc_alloc_Global"));
        assert!(!res.contains("struct _core_marker_PhantomData"));
        // В RealData должно остаться только 2 поля, ZST PhantomData вырезано
        assert!(res.contains("struct RealData {\n    field0: i64,\n    field1: *u8,\n}"));
        // Интринсики lifetime и assume удалены, memcpy транслирован в memcpy(...)
        assert!(!res.contains("llvm_lifetime"));
        assert!(!res.contains("llvm_assume"));
        assert!(res.contains("memcpy("));
    }

    #[test]
    fn test_arbitrary_bitwidths_and_anon_structs() {
        let ir = r#"
declare { ptr, i64 } @slice_func(i24 %custom_int)

@global_table = private unnamed_addr constant [4 x i8] c"test", align 1

define { ptr, i64 } @call_slice(i24 %val) {
entry:
  %res = call { ptr, i64 } @slice_func(i24 %val)
  ret { ptr, i64 } %res
}
"#;
        let res = transpile_llvm_ir(ir, &LlvmToGorawOptions::default()).unwrap();
        assert!(res.contains("val: i32") || res.contains("arg0: i32"));
        assert!(res.contains("struct Anon_ptr_u8_i64 {"));
        assert!(res.contains("extern fn slice_func(custom_int: i32) -> Anon_ptr_u8_i64;"));
    }
}
