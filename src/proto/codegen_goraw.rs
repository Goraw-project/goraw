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

// --- reader (decode) ---
struct GpbReader { data: *u8, len: i64, pos: i64 }

fn gpb_rd_new(data: *u8, len: i64) -> GpbReader {
    return GpbReader { data: data, len: len, pos: 0 };
}

fn gpb_rd_eof(r: *mut GpbReader) -> bool {
    unsafe { return r.pos >= r.len; }
}

fn gpb_rd_byte(r: *mut GpbReader) -> u8 {
    unsafe {
        let b: u8 = r.data[r.pos];
        r.pos = r.pos + 1;
        return b;
    }
}

fn gpb_rd_varint(r: *mut GpbReader) -> u64 {
    let mut result: u64 = 0;
    let mut shift: u64 = 0;
    for {
        let b: u8 = gpb_rd_byte(r);
        result = result | (((b & 127) as u64) << shift);
        if (b & 128) == 0 { break; }
        shift = shift + 7;
    }
    return result;
}

fn gpb_rd_fixed32(r: *mut GpbReader) -> u32 {
    let b0: u32 = gpb_rd_byte(r) as u32;
    let b1: u32 = gpb_rd_byte(r) as u32;
    let b2: u32 = gpb_rd_byte(r) as u32;
    let b3: u32 = gpb_rd_byte(r) as u32;
    return b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
}

fn gpb_rd_fixed64(r: *mut GpbReader) -> u64 {
    let mut result: u64 = 0;
    let mut shift: u64 = 0;
    let mut i: i64 = 0;
    while i < 8 {
        result = result | ((gpb_rd_byte(r) as u64) << shift);
        shift = shift + 8;
        i = i + 1;
    }
    return result;
}

fn gpb_rd_skip(r: *mut GpbReader, wt: u64) {
    if wt == 0 {
        gpb_rd_varint(r);
    } else if wt == 1 {
        unsafe { r.pos = r.pos + 8; }
    } else if wt == 5 {
        unsafe { r.pos = r.pos + 4; }
    } else if wt == 2 {
        let n: i64 = gpb_rd_varint(r) as i64;
        unsafe { r.pos = r.pos + n; }
    }
}

fn gpb_unzigzag32(v: u64) -> i32 {
    let u: u32 = v as u32;
    return ((u >> 1) as i32) ^ (0 - ((u & 1) as i32));
}

fn gpb_unzigzag64(v: u64) -> i64 {
    return ((v >> 1) as i64) ^ (0 - ((v & 1) as i64));
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
    for m in &file.messages {
        gen_decode(&mut out, file, m);
        out.push('\n');
    }
    out
}

/// Способ чтения скаляра из reader'а.
enum ReadKind {
    Varint, // let v: u64 = gpb_rd_varint(...)
    F32,    // let v: u32 = gpb_rd_fixed32(...)
    F64,    // let v: u64 = gpb_rd_fixed64(...)
}

/// (способ чтения, выражение конверсии из прочитанного `v` в тип поля).
fn scalar_read(file: &FileD, ty: &FieldType) -> (ReadKind, String) {
    use FieldType::*;
    if is_enum(file, ty) {
        return (ReadKind::Varint, "v as i32".into());
    }
    match ty {
        Int32 => (ReadKind::Varint, "v as i32".into()),
        Int64 => (ReadKind::Varint, "v as i64".into()),
        UInt32 => (ReadKind::Varint, "v as u32".into()),
        UInt64 => (ReadKind::Varint, "v".into()),
        SInt32 => (ReadKind::Varint, "gpb_unzigzag32(v)".into()),
        SInt64 => (ReadKind::Varint, "gpb_unzigzag64(v)".into()),
        Bool => (ReadKind::Varint, "v != 0".into()),
        Fixed32 => (ReadKind::F32, "v".into()),
        SFixed32 => (ReadKind::F32, "v as i32".into()),
        Float => (ReadKind::F32, "f32_from_bits(v)".into()),
        Fixed64 => (ReadKind::F64, "v".into()),
        SFixed64 => (ReadKind::F64, "v as i64".into()),
        Double => (ReadKind::F64, "f64_from_bits(v)".into()),
        _ => (ReadKind::Varint, "v".into()),
    }
}

fn read_stmt(kind: &ReadKind) -> &'static str {
    match kind {
        ReadKind::Varint => "let v: u64 = gpb_rd_varint(&mut r);",
        ReadKind::F32 => "let v: u32 = gpb_rd_fixed32(&mut r);",
        ReadKind::F64 => "let v: u64 = gpb_rd_fixed64(&mut r);",
    }
}

fn gen_decode(out: &mut String, file: &FileD, m: &MessageD) {
    let _ = writeln!(out, "fn decode_{}(data: *u8, len: i64) -> {} {{", m.name, m.name);
    let _ = writeln!(out, "    let mut m: {} = zeroed();", m.name);
    let _ = writeln!(out, "    let mut r: GpbReader = gpb_rd_new(data, len);");
    let _ = writeln!(out, "    unsafe {{");
    let _ = writeln!(out, "        for {{");
    let _ = writeln!(out, "            if gpb_rd_eof(&mut r) {{ break; }}");
    let _ = writeln!(out, "            let tag: u64 = gpb_rd_varint(&mut r);");
    let _ = writeln!(out, "            let fnum: u64 = tag >> 3;");
    let _ = writeln!(out, "            let wt: u64 = tag & 7;");

    let mut first = true;
    for f in &m.fields {
        if matches!(f.ty, FieldType::Map(..)) {
            continue;
        }
        // repeated message / repeated string decode — PB2, попадут в skip.
        let handled = if f.repeated {
            scalar_gtype(file, &f.ty).is_some()
        } else {
            scalar_gtype(file, &f.ty).is_some()
                || matches!(f.ty, FieldType::String | FieldType::Bytes)
                || is_message(file, &f.ty)
        };
        if !handled {
            continue;
        }
        let kw = if first { "if" } else { "} else if" };
        first = false;
        let _ = writeln!(out, "            {kw} fnum == {} {{", f.number);
        if f.repeated {
            gen_decode_packed(out, file, f);
        } else {
            gen_decode_singular(out, file, f);
        }
    }
    if first {
        // ни одного известного поля — просто skip
        let _ = writeln!(out, "            gpb_rd_skip(&mut r, wt);");
    } else {
        let _ = writeln!(out, "            }} else {{ gpb_rd_skip(&mut r, wt); }}");
    }
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "    return m;");
    let _ = writeln!(out, "}}");
}

fn gen_decode_singular(out: &mut String, file: &FileD, f: &FieldD) {
    let name = &f.name;
    let set_has = |out: &mut String| {
        if f.presence == Presence::Explicit {
            let _ = writeln!(out, "                m.has_{name} = true;");
        }
    };

    if scalar_gtype(file, &f.ty).is_some() {
        let (kind, conv) = scalar_read(file, &f.ty);
        let _ = writeln!(out, "                {}", read_stmt(&kind));
        let _ = writeln!(out, "                m.{name} = {conv};");
        set_has(out);
        return;
    }
    if matches!(f.ty, FieldType::String | FieldType::Bytes) {
        let _ = writeln!(out, "                let ln: i64 = gpb_rd_varint(&mut r) as i64;");
        let _ = writeln!(out, "                m.{name}_ptr = &r.data[r.pos];");
        let _ = writeln!(out, "                m.{name}_len = ln;");
        let _ = writeln!(out, "                r.pos = r.pos + ln;");
        set_has(out);
        return;
    }
    if is_message(file, &f.ty) {
        let sub = flat(&f.ty);
        let _ = writeln!(out, "                let ln: i64 = gpb_rd_varint(&mut r) as i64;");
        let _ = writeln!(out, "                let subp_{name}: *u8 = &r.data[r.pos];");
        let _ = writeln!(out, "                let sp_{name}: *mut {sub} = alloc(sizeof({sub})) as *mut {sub};");
        let _ = writeln!(out, "                *sp_{name} = decode_{sub}(subp_{name}, ln);");
        let _ = writeln!(out, "                m.{name} = sp_{name};");
        let _ = writeln!(out, "                r.pos = r.pos + ln;");
    }
}

/// Декодирование packed repeated числовых полей (два прохода: подсчёт → заполнение).
fn gen_decode_packed(out: &mut String, file: &FileD, f: &FieldD) {
    let name = &f.name;
    let et = scalar_gtype(file, &f.ty).unwrap();
    let (kind, conv) = scalar_read(file, &f.ty);

    let _ = writeln!(out, "                let plen: i64 = gpb_rd_varint(&mut r) as i64;");
    let _ = writeln!(out, "                let start_{name}: i64 = r.pos;");
    let _ = writeln!(out, "                let end_{name}: i64 = start_{name} + plen;");

    // Подсчёт количества элементов.
    match kind {
        ReadKind::Varint => {
            let _ = writeln!(out, "                let mut cnt_{name}: i64 = 0;");
            let _ = writeln!(out, "                let mut j_{name}: i64 = start_{name};");
            let _ = writeln!(out, "                while j_{name} < end_{name} {{");
            let _ = writeln!(out, "                    if (r.data[j_{name}] & 128) == 0 {{ cnt_{name} = cnt_{name} + 1; }}");
            let _ = writeln!(out, "                    j_{name} = j_{name} + 1;");
            let _ = writeln!(out, "                }}");
        }
        ReadKind::F32 => {
            let _ = writeln!(out, "                let cnt_{name}: i64 = plen / 4;");
        }
        ReadKind::F64 => {
            let _ = writeln!(out, "                let cnt_{name}: i64 = plen / 8;");
        }
    }

    // Аллокация и заполнение.
    let _ = writeln!(out, "                let raw_{name}: *mut {et} = alloc(cnt_{name} * sizeof({et})) as *mut {et};");
    let _ = writeln!(out, "                let mut k_{name}: i64 = 0;");
    let _ = writeln!(out, "                while k_{name} < cnt_{name} {{");
    let _ = writeln!(out, "                    {}", read_stmt(&kind));
    let _ = writeln!(out, "                    raw_{name}[k_{name}] = {conv};");
    let _ = writeln!(out, "                    k_{name} = k_{name} + 1;");
    let _ = writeln!(out, "                }}");
    let _ = writeln!(out, "                m.{name} = make_slice(raw_{name}, cnt_{name});");
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
