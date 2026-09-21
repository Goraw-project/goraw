//! Кодоген protobuf → **Goraw**. Генерирует: prelude (растущий буфер на куче +
//! varint/zigzag/fixed/bytes-хелперы) и по каждому сообщению — структуру полей
//! и функцию `encode_<Msg>(m: *<Msg>, b: *mut GpbBuf)`, дающую байты,
//! идентичные wire-формату protobuf.
//!
//! PB1: encode для скаляров (все числовые, bool, enum, fixed/float/double),
//! string/bytes, вложенных сообщений; repeated (packed для числовых, expanded
//! для сообщений). Пропущены (PB2): map, oneof-семантика, repeated string,
//! decode.

use crate::proto::ast::FieldType;
use crate::proto::descriptor::{FieldD, FileD, MessageD, Presence};
use std::fmt::Write as _;

const PRELUDE: &str = r#"// --- gorawpb runtime prelude (сгенерировано) ---
struct GpbBuf { data: *mut u8, len: i64, cap: i64 }

fn gpb_buf_new() -> GpbBuf {
    let cap: i64 = 16;
    return GpbBuf { data: alloc(cap), len: 0, cap: cap };
}

fn gpb_push(b: *mut GpbBuf, x: u8) {
    unsafe {
        if b.len == b.cap {
            let nc: i64 = b.cap * 2;
            b.data = realloc(b.data, nc);
            b.cap = nc;
        }
        b.data[b.len] = x;
        b.len = b.len + 1;
    }
}

fn gpb_varint(b: *mut GpbBuf, v: u64) {
    let mut x: u64 = v;
    while x >= 128 {
        gpb_push(b, ((x & 127) | 128) as u8);
        x = x >> 7;
    }
    gpb_push(b, x as u8);
}

fn gpb_tag(b: *mut GpbBuf, field: u64, wt: u64) {
    gpb_varint(b, (field << 3) | wt);
}

fn gpb_zigzag32(v: i32) -> u64 {
    let z: i32 = (v << 1) ^ (v >> 31);
    return z as u32 as u64;
}

fn gpb_zigzag64(v: i64) -> u64 {
    return ((v << 1) ^ (v >> 63)) as u64;
}

fn gpb_fixed32(b: *mut GpbBuf, v: u32) {
    gpb_push(b, (v & 255) as u8);
    gpb_push(b, ((v >> 8) & 255) as u8);
    gpb_push(b, ((v >> 16) & 255) as u8);
    gpb_push(b, ((v >> 24) & 255) as u8);
}

fn gpb_fixed64(b: *mut GpbBuf, v: u64) {
    let mut x: u64 = v;
    let mut i: i64 = 0;
    while i < 8 {
        gpb_push(b, (x & 255) as u8);
        x = x >> 8;
        i = i + 1;
    }
}

fn gpb_bytes(b: *mut GpbBuf, data: *u8, len: i64) {
    gpb_varint(b, len as u64);
    let mut i: i64 = 0;
    while i < len {
        unsafe { gpb_push(b, data[i]); }
        i = i + 1;
    }
}
// --- конец prelude ---
"#;

const WT_VARINT: u64 = 0;
const WT_I64: u64 = 1;
const WT_LEN: u64 = 2;
const WT_I32: u64 = 5;

pub fn generate(file: &FileD) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "// Сгенерировано gorawpb из .proto (edition {}). НЕ РЕДАКТИРОВАТЬ.", file.edition);
    out.push('\n');
    out.push_str(PRELUDE);
    out.push('\n');

    for m in &file.messages {
        gen_struct(&mut out, file, m);
        out.push('\n');
    }
    for m in &file.messages {
        gen_encode(&mut out, file, m);
        out.push('\n');
    }
    out
}

fn is_enum(file: &FileD, ty: &FieldType) -> bool {
    matches!(ty, FieldType::Named(n) if file.enum_names.contains(simple(n)))
}
fn is_message(file: &FileD, ty: &FieldType) -> bool {
    matches!(ty, FieldType::Named(n) if file.message_names.contains(simple(n)))
}
fn simple(n: &str) -> &str {
    n.rsplit('.').next().unwrap_or(n)
}

/// Goraw-тип для singular-скаляра (или None для string/bytes/message/map).
fn scalar_gtype(file: &FileD, ty: &FieldType) -> Option<&'static str> {
    use FieldType::*;
    if is_enum(file, ty) {
        return Some("i32");
    }
    Some(match ty {
        Int32 | SInt32 | SFixed32 => "i32",
        Int64 | SInt64 | SFixed64 => "i64",
        UInt32 | Fixed32 => "u32",
        UInt64 | Fixed64 => "u64",
        Bool => "bool",
        Float => "f32",
        Double => "f64",
        _ => return None,
    })
}

fn gen_struct(out: &mut String, file: &FileD, m: &MessageD) {
    let _ = writeln!(out, "struct {} {{", m.name);
    for f in &m.fields {
        if matches!(f.ty, FieldType::Map(..)) {
            let _ = writeln!(out, "    // map-поле `{}` — PB2", f.name);
            continue;
        }
        if f.repeated {
            if let Some(g) = scalar_gtype(file, &f.ty) {
                let _ = writeln!(out, "    {}: []{},", f.name, g);
            } else if is_message(file, &f.ty) {
                let _ = writeln!(out, "    {}: []*{},", f.name, flat(&f.ty));
            } else {
                let _ = writeln!(out, "    // repeated string/bytes `{}` — PB2", f.name);
            }
            continue;
        }
        // singular
        if let Some(g) = scalar_gtype(file, &f.ty) {
            let _ = writeln!(out, "    {}: {},", f.name, g);
            if f.presence == Presence::Explicit {
                let _ = writeln!(out, "    has_{}: bool,", f.name);
            }
        } else if matches!(f.ty, FieldType::String | FieldType::Bytes) {
            let _ = writeln!(out, "    {}_ptr: *u8,", f.name);
            let _ = writeln!(out, "    {}_len: i64,", f.name);
            if f.presence == Presence::Explicit {
                let _ = writeln!(out, "    has_{}: bool,", f.name);
            }
        } else if is_message(file, &f.ty) {
            let _ = writeln!(out, "    {}: *{},", f.name, flat(&f.ty));
        }
    }
    let _ = writeln!(out, "}}");
}

fn flat(ty: &FieldType) -> String {
    match ty {
        FieldType::Named(n) => simple(n).to_string(),
        _ => "void".to_string(),
    }
}

fn gen_encode(out: &mut String, file: &FileD, m: &MessageD) {
    let _ = writeln!(out, "fn encode_{}(m: *{}, b: *mut GpbBuf) {{", m.name, m.name);
    let _ = writeln!(out, "    unsafe {{");
    for f in &m.fields {
        if matches!(f.ty, FieldType::Map(..)) {
            continue;
        }
        if f.repeated {
            gen_repeated(out, file, f);
        } else {
            gen_singular(out, file, f);
        }
    }
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
}

fn gen_singular(out: &mut String, file: &FileD, f: &FieldD) {
    let n = f.number as u64;
    let base = format!("m.{}", f.name);
    let enum_field = is_enum(file, &f.ty);

    // Скаляр (числовой/bool/enum/fixed/float/double).
    if scalar_gtype(file, &f.ty).is_some() {
        let guard = presence_guard(f, &base);
        let (wt, valexpr) = scalar_wire(&f.ty, enum_field, &base);
        emit_guarded(out, &guard, &format!("gpb_tag(b, {n}, {wt}); {};", write_call(wt, &valexpr)));
        return;
    }
    // string / bytes.
    if matches!(f.ty, FieldType::String | FieldType::Bytes) {
        let guard = if f.presence == Presence::Explicit {
            format!("if m.has_{} {{", f.name)
        } else {
            format!("if m.{}_len != 0 {{", f.name)
        };
        let body = format!(
            "gpb_tag(b, {n}, {WT_LEN}); gpb_bytes(b, m.{name}_ptr, m.{name}_len);",
            name = f.name
        );
        let _ = writeln!(out, "        {guard} {body} }}");
        return;
    }
    // вложенное сообщение (length-delimited).
    if is_message(file, &f.ty) {
        let sub = flat(&f.ty);
        let name = &f.name;
        let _ = writeln!(out, "        if (m.{name} as i64) != 0 {{");
        let _ = writeln!(out, "            let mut sub_{name} = gpb_buf_new();");
        let _ = writeln!(out, "            encode_{sub}(m.{name}, &mut sub_{name});");
        let _ = writeln!(out, "            gpb_tag(b, {n}, {WT_LEN});");
        let _ = writeln!(out, "            gpb_bytes(b, sub_{name}.data as *u8, sub_{name}.len);");
        let _ = writeln!(out, "            free(sub_{name}.data);");
        let _ = writeln!(out, "        }}");
    }
}

fn gen_repeated(out: &mut String, file: &FileD, f: &FieldD) {
    let n = f.number as u64;
    let name = &f.name;
    let enum_field = is_enum(file, &f.ty);

    // repeated message — всегда expanded, каждый элемент length-delimited.
    if is_message(file, &f.ty) {
        let sub = flat(&f.ty);
        let _ = writeln!(out, "        for i_{name} := 0; i_{name} < m.{name}.len; i_{name}++ {{");
        let _ = writeln!(out, "            let mut e_{name} = gpb_buf_new();");
        let _ = writeln!(out, "            encode_{sub}(m.{name}[i_{name}], &mut e_{name});");
        let _ = writeln!(out, "            gpb_tag(b, {n}, {WT_LEN});");
        let _ = writeln!(out, "            gpb_bytes(b, e_{name}.data as *u8, e_{name}.len);");
        let _ = writeln!(out, "            free(e_{name}.data);");
        let _ = writeln!(out, "        }}");
        return;
    }
    // repeated скаляр.
    if scalar_gtype(file, &f.ty).is_none() {
        // repeated string/bytes/map — PB2
        return;
    }
    let elem = format!("m.{name}[i_{name}]");
    let (wt, valexpr) = scalar_wire(&f.ty, enum_field, &elem);

    if f.packed {
        // packed: длина-делимитированная упаковка значений.
        let _ = writeln!(out, "        if m.{name}.len != 0 {{");
        let _ = writeln!(out, "            let mut pk_{name} = gpb_buf_new();");
        let _ = writeln!(out, "            for i_{name} := 0; i_{name} < m.{name}.len; i_{name}++ {{");
        let _ = writeln!(out, "                {}", write_call_to(wt, "&mut pk_{name}", &valexpr).replace("{name}", name));
        let _ = writeln!(out, "            }}");
        let _ = writeln!(out, "            gpb_tag(b, {n}, {WT_LEN});");
        let _ = writeln!(out, "            gpb_bytes(b, pk_{name}.data as *u8, pk_{name}.len);");
        let _ = writeln!(out, "            free(pk_{name}.data);");
        let _ = writeln!(out, "        }}");
    } else {
        // expanded: тег + значение на каждый элемент.
        let _ = writeln!(out, "        for i_{name} := 0; i_{name} < m.{name}.len; i_{name}++ {{");
        let _ = writeln!(out, "            gpb_tag(b, {n}, {wt}); {};", write_call(wt, &valexpr));
        let _ = writeln!(out, "        }}");
    }
}

/// Guard по presence для скалярных полей.
fn presence_guard(f: &FieldD, base: &str) -> String {
    match f.presence {
        Presence::Explicit => format!("if m.has_{} {{", f.name),
        _ => {
            // implicit: пишем если не нулевое
            if f.ty == FieldType::Bool {
                format!("if {base} {{")
            } else if matches!(f.ty, FieldType::Float | FieldType::Double) {
                format!("if {base} != 0.0 {{")
            } else {
                format!("if {base} != 0 {{")
            }
        }
    }
}

fn emit_guarded(out: &mut String, guard: &str, body: &str) {
    let _ = writeln!(out, "        {guard} {body} }}");
}

/// (wire type, выражение-значение) для скаляра по базовому доступу `base`.
fn scalar_wire(ty: &FieldType, is_enum: bool, base: &str) -> (u64, String) {
    use FieldType::*;
    if is_enum {
        return (WT_VARINT, format!("{base} as i64 as u64"));
    }
    match ty {
        Int32 => (WT_VARINT, format!("{base} as i64 as u64")),
        Int64 => (WT_VARINT, format!("{base} as u64")),
        UInt32 => (WT_VARINT, format!("{base} as u64")),
        UInt64 => (WT_VARINT, base.to_string()),
        SInt32 => (WT_VARINT, format!("gpb_zigzag32({base})")),
        SInt64 => (WT_VARINT, format!("gpb_zigzag64({base})")),
        Bool => (WT_VARINT, format!("{base} as u64")),
        Fixed32 => (WT_I32, base.to_string()),
        SFixed32 => (WT_I32, format!("{base} as u32")),
        Float => (WT_I32, format!("f32_bits({base})")),
        Fixed64 => (WT_I64, base.to_string()),
        SFixed64 => (WT_I64, format!("{base} as u64")),
        Double => (WT_I64, format!("f64_bits({base})")),
        _ => (WT_VARINT, base.to_string()),
    }
}

/// Вызов записи значения в основной буфер `b` по wire type.
fn write_call(wt: u64, valexpr: &str) -> String {
    match wt {
        WT_I32 => format!("gpb_fixed32(b, {valexpr})"),
        WT_I64 => format!("gpb_fixed64(b, {valexpr})"),
        _ => format!("gpb_varint(b, {valexpr})"),
    }
}
/// Вызов записи в произвольный буфер `buf` (для packed).
fn write_call_to(wt: u64, buf: &str, valexpr: &str) -> String {
    match wt {
        WT_I32 => format!("gpb_fixed32({buf}, {valexpr});"),
        WT_I64 => format!("gpb_fixed64({buf}, {valexpr});"),
        _ => format!("gpb_varint({buf}, {valexpr});"),
    }
}
