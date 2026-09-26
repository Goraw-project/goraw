//! Кодоген protobuf → **Goraw**. Генерирует: prelude (растущий буфер на куче +
//! varint/zigzag/fixed/bytes-хелперы, векторы строк и указателей) и по каждому сообщению:
//! структуры полей, структуры map-записей, константы oneof,
//! `encode_<Msg>(m: *<Msg>, b: *mut GpbBuf)` и `decode_<Msg>(data: *u8, len: i64) -> <Msg>`.
//!
//! Полная поддержка PB2:
//! - Скаляры (все числовые, bool, enum, fixed/float/double)
//! - Singular string / bytes
//! - Repeated скаляры (packed / expanded)
//! - Repeated string / bytes ([]GpbString)
//! - Repeated message ([]*Sub)
//! - Oneof (which_<name> дискриминатор + именованные константы)
//! - Map<K, V> (flat []Entry слайсы, wire-совместимые со спекой protobuf)

use crate::proto::ast::FieldType;
use crate::proto::descriptor::{FieldD, FileD, MessageD, OneofD, Presence, ServiceD};
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

// --- string / slice representation ---
struct GpbString { data: *u8, len: i64 }

fn gpb_str(s: str) -> GpbString {
    unsafe {
        if s.len == 0 {
            return GpbString { data: null, len: 0 };
        }
        return GpbString { data: &s[0], len: s.len };
    }
}

struct GpbStringVec { data: *mut GpbString, len: i64, cap: i64 }

fn gpb_strvec_new() -> GpbStringVec {
    let cap: i64 = 8;
    return GpbStringVec { data: alloc(cap * sizeof(GpbString)) as *mut GpbString, len: 0, cap: cap };
}

fn gpb_strvec_push(v: *mut GpbStringVec, s: GpbString) {
    unsafe {
        if v.len == v.cap {
            let nc: i64 = v.cap * 2;
            v.data = realloc(v.data as *mut u8, nc * sizeof(GpbString)) as *mut GpbString;
            v.cap = nc;
        }
        v.data[v.len] = s;
        v.len = v.len + 1;
    }
}

struct GpbPtrVec { data: *mut *u8, len: i64, cap: i64 }

fn gpb_ptrvec_new() -> GpbPtrVec {
    let cap: i64 = 8;
    return GpbPtrVec { data: alloc(cap * 8) as *mut *u8, len: 0, cap: cap };
}

fn gpb_ptrvec_push(v: *mut GpbPtrVec, p: *u8) {
    unsafe {
        if v.len == v.cap {
            let nc: i64 = v.cap * 2;
            v.data = realloc(v.data as *mut u8, nc * 8) as *mut *u8;
            v.cap = nc;
        }
        v.data[v.len] = p;
        v.len = v.len + 1;
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
    generate_ext(file, true)
}

pub fn generate_ext(file: &FileD, include_prelude: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "// Сгенерировано gorawpb из .proto (edition {}). НЕ РЕДАКТИРОВАТЬ.", file.edition);
    out.push('\n');
    if include_prelude {
        out.push_str(PRELUDE);
        out.push('\n');
    }

    // 1. Генерация вспомогательных структур и векторов для Map-полей
    for m in &file.messages {
        for f in &m.fields {
            if let FieldType::Map(k, v) = &f.ty {
                gen_map_definitions(&mut out, file, &m.name, f, k, v);
                out.push('\n');
            }
        }
    }

    // 2. Генерация именованных констант для Oneof
    for m in &file.messages {
        for oo in &m.oneofs {
            gen_oneof_constants(&mut out, &m.name, oo);
            out.push('\n');
        }
    }

    // 3. Структуры сообщений
    for m in &file.messages {
        gen_struct(&mut out, file, m);
        out.push('\n');
    }

    // 4. Функции encode_<Msg>
    for m in &file.messages {
        gen_encode(&mut out, file, m);
        out.push('\n');
    }

    // 5. Функции decode_<Msg>
    for m in &file.messages {
        gen_decode(&mut out, file, m);
        out.push('\n');
    }

    // 6. Сервисы RPC (клиентские и серверные хелперы + пути)
    for s in &file.services {
        gen_service(&mut out, file, s);
        out.push('\n');
    }

    out
}

fn gen_service(out: &mut String, _file: &FileD, s: &ServiceD) {
    let _ = writeln!(out, "// --- RPC Service: {} ---", s.name);
    let s_upper = s.name.to_uppercase();
    let _ = writeln!(out, "const RPC_{s_upper}_METHOD_COUNT: i32 = {};", s.methods.len());

    for (idx, m) in s.methods.iter().enumerate() {
        let id = idx + 1;
        let m_upper = m.name.to_uppercase();
        let _ = writeln!(out, "const RPC_{s_upper}_{m_upper}_ID: i32 = {id};");
        let _ = writeln!(out, "fn rpc_{}_{}_path() -> str {{ return \"/{}/{}\"; }}", s.name, m.name, s.name, m.name);

        // Client helpers
        let _ = writeln!(out, "fn rpc_{}_{}_encode_request(req: *{}, out_buf: *mut GpbBuf) {{", s.name, m.name, m.input_type);
        let _ = writeln!(out, "    encode_{}(req, out_buf);", m.input_type);
        let _ = writeln!(out, "}}");

        let _ = writeln!(out, "fn rpc_{}_{}_decode_response(data: *u8, len: i64) -> {} {{", s.name, m.name, m.output_type);
        let _ = writeln!(out, "    return decode_{}(data, len);", m.output_type);
        let _ = writeln!(out, "}}");

        // Server helpers
        let _ = writeln!(out, "fn rpc_{}_{}_decode_request(data: *u8, len: i64) -> {} {{", s.name, m.name, m.input_type);
        let _ = writeln!(out, "    return decode_{}(data, len);", m.input_type);
        let _ = writeln!(out, "}}");

        let _ = writeln!(out, "fn rpc_{}_{}_encode_response(resp: *{}, out_buf: *mut GpbBuf) {{", s.name, m.name, m.output_type);
        let _ = writeln!(out, "    encode_{}(resp, out_buf);", m.output_type);
        let _ = writeln!(out, "}}");
    }

    // Method dispatch identifier helper
    let _ = writeln!(out, "fn rpc_{}_method_id(method: str) -> i32 {{", s.name);
    for (idx, m) in s.methods.iter().enumerate() {
        let id = idx + 1;
        let _ = writeln!(out, "    if method == \"{}\" || method == \"/{}/{}\" {{", m.name, s.name, m.name);
        let _ = writeln!(out, "        return {id};");
        let _ = writeln!(out, "    }}");
    }
    let _ = writeln!(out, "    return 0;");
    let _ = writeln!(out, "}}");

    // Method path helper
    let _ = writeln!(out, "fn rpc_{}_method_path(id: i32) -> str {{", s.name);
    for (idx, m) in s.methods.iter().enumerate() {
        let id = idx + 1;
        let _ = writeln!(out, "    if id == {id} {{");
        let _ = writeln!(out, "        return \"/{}/{}\";", s.name, m.name);
        let _ = writeln!(out, "    }}");
    }
    let _ = writeln!(out, "    return \"\";");
    let _ = writeln!(out, "}}");
}

fn to_pascal_case(s: &str) -> String {
    let mut res = String::new();
    let mut cap = true;
    for c in s.chars() {
        if c == '_' {
            cap = true;
        } else if cap {
            res.extend(c.to_uppercase());
            cap = false;
        } else {
            res.push(c);
        }
    }
    res
}

fn map_entry_name(msg_name: &str, field_name: &str) -> String {
    format!("{}_{}_Entry", msg_name, to_pascal_case(field_name))
}

fn map_vec_name(msg_name: &str, field_name: &str) -> String {
    format!("{}_{}_Vec", msg_name, to_pascal_case(field_name))
}

fn map_vec_fn(msg_name: &str, field_name: &str) -> String {
    format!("{}_{}_vec", msg_name.to_lowercase(), field_name.to_lowercase())
}

fn map_elem_gtype(file: &FileD, ty: &FieldType) -> String {
    if let Some(s) = scalar_gtype(file, ty) {
        return s.to_string();
    }
    if matches!(ty, FieldType::String | FieldType::Bytes) {
        return "GpbString".to_string();
    }
    if is_message(file, ty) {
        return format!("*{}", flat(ty));
    }
    "i32".to_string()
}

fn gen_map_definitions(
    out: &mut String,
    file: &FileD,
    msg_name: &str,
    f: &FieldD,
    k: &FieldType,
    v: &FieldType,
) {
    let entry_name = map_entry_name(msg_name, &f.name);
    let vec_name = map_vec_name(msg_name, &f.name);
    let vec_fn = map_vec_fn(msg_name, &f.name);
    let k_ty = map_elem_gtype(file, k);
    let v_ty = map_elem_gtype(file, v);

    let _ = writeln!(out, "struct {entry_name} {{");
    let _ = writeln!(out, "    key: {k_ty},");
    let _ = writeln!(out, "    value: {v_ty},");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let _ = writeln!(out, "struct {vec_name} {{");
    let _ = writeln!(out, "    data: *mut {entry_name},");
    let _ = writeln!(out, "    len: i64,");
    let _ = writeln!(out, "    cap: i64,");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let _ = writeln!(out, "fn {vec_fn}_new() -> {vec_name} {{");
    let _ = writeln!(out, "    let cap: i64 = 8;");
    let _ = writeln!(out, "    return {vec_name} {{");
    let _ = writeln!(out, "        data: alloc(cap * sizeof({entry_name})) as *mut {entry_name},");
    let _ = writeln!(out, "        len: 0,");
    let _ = writeln!(out, "        cap: cap,");
    let _ = writeln!(out, "    }};");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let _ = writeln!(out, "fn {vec_fn}_push(v: *mut {vec_name}, item: {entry_name}) {{");
    let _ = writeln!(out, "    unsafe {{");
    let _ = writeln!(out, "        if v.len == v.cap {{");
    let _ = writeln!(out, "            let nc: i64 = v.cap * 2;");
    let _ = writeln!(out, "            v.data = realloc(v.data as *mut u8, nc * sizeof({entry_name})) as *mut {entry_name};");
    let _ = writeln!(out, "            v.cap = nc;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "        v.data[v.len] = item;");
    let _ = writeln!(out, "        v.len = v.len + 1;");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
}

fn gen_oneof_constants(out: &mut String, msg_name: &str, oo: &OneofD) {
    let msg_up = msg_name.to_uppercase();
    let oo_up = oo.name.to_uppercase();
    let _ = writeln!(out, "const {msg_up}_{oo_up}_NOT_SET: i32 = 0;");
    for f in &oo.fields {
        let f_up = f.name.to_uppercase();
        let _ = writeln!(out, "const {msg_up}_{oo_up}_{f_up}: i32 = {};", f.number);
    }
}

/// Способ чтения скаляра из reader'а.
enum ReadKind {
    Varint,
    F32,
    F64,
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

fn read_stmt_with_reader(reader: &str, kind: &ReadKind) -> String {
    match kind {
        ReadKind::Varint => format!("let v: u64 = gpb_rd_varint(&mut {reader});"),
        ReadKind::F32 => format!("let v: u32 = gpb_rd_fixed32(&mut {reader});"),
        ReadKind::F64 => format!("let v: u64 = gpb_rd_fixed64(&mut {reader});"),
    }
}

fn gen_decode(out: &mut String, file: &FileD, m: &MessageD) {
    let _ = writeln!(out, "fn decode_{}(data: *u8, len: i64) -> {} {{", m.name, m.name);
    let _ = writeln!(out, "    let mut m: {} = zeroed();", m.name);
    let _ = writeln!(out, "    let mut r: GpbReader = gpb_rd_new(data, len);");
    let _ = writeln!(out, "    unsafe {{");

    // Инициализация локальных векторов для repeated и map
    for f in &m.fields {
        if matches!(f.ty, FieldType::Map(..)) {
            let vec_fn = map_vec_fn(&m.name, &f.name);
            let vec_ty = map_vec_name(&m.name, &f.name);
            let _ = writeln!(out, "        let mut vec_{}: {} = {}_new();", f.name, vec_ty, vec_fn);
        } else if f.repeated {
            if matches!(f.ty, FieldType::String | FieldType::Bytes) {
                let _ = writeln!(out, "        let mut vec_{}: GpbStringVec = gpb_strvec_new();", f.name);
            } else if is_message(file, &f.ty) {
                let _ = writeln!(out, "        let mut vec_{}: GpbPtrVec = gpb_ptrvec_new();", f.name);
            }
        }
    }

    let _ = writeln!(out, "        for {{");
    let _ = writeln!(out, "            if gpb_rd_eof(&mut r) {{ break; }}");
    let _ = writeln!(out, "            let tag: u64 = gpb_rd_varint(&mut r);");
    let _ = writeln!(out, "            let fnum: u64 = tag >> 3;");
    let _ = writeln!(out, "            let wt: u64 = tag & 7;");

    let mut first = true;

    // 1. Обычные поля
    for f in &m.fields {
        let kw = if first { "if" } else { "} else if" };
        first = false;
        let _ = writeln!(out, "            {kw} fnum == {} {{", f.number);

        if let FieldType::Map(k, v) = &f.ty {
            gen_decode_map(out, file, &m.name, f, k, v);
        } else if f.repeated {
            if matches!(f.ty, FieldType::String | FieldType::Bytes) {
                let _ = writeln!(out, "                let ln: i64 = gpb_rd_varint(&mut r) as i64;");
                let _ = writeln!(out, "                let s: GpbString = GpbString {{ data: &r.data[r.pos], len: ln }};");
                let _ = writeln!(out, "                gpb_strvec_push(&mut vec_{}, s);", f.name);
                let _ = writeln!(out, "                r.pos = r.pos + ln;");
            } else if is_message(file, &f.ty) {
                let sub = flat(&f.ty);
                let _ = writeln!(out, "                let ln: i64 = gpb_rd_varint(&mut r) as i64;");
                let _ = writeln!(out, "                let subp_{name}: *u8 = &r.data[r.pos];", name = f.name);
                let _ = writeln!(out, "                let sp_{name}: *mut {sub} = alloc(sizeof({sub})) as *mut {sub};", name = f.name);
                let _ = writeln!(out, "                *sp_{name} = decode_{sub}(subp_{name}, ln);", name = f.name);
                let _ = writeln!(out, "                gpb_ptrvec_push(&mut vec_{}, sp_{} as *u8);", f.name, f.name);
                let _ = writeln!(out, "                r.pos = r.pos + ln;");
            } else {
                gen_decode_packed(out, file, f);
            }
        } else {
            gen_decode_singular(out, file, f);
        }
    }

    // 2. Oneof поля
    for oo in &m.oneofs {
        for f in &oo.fields {
            let kw = if first { "if" } else { "} else if" };
            first = false;
            let _ = writeln!(out, "            {kw} fnum == {} {{", f.number);
            gen_decode_oneof_field(out, file, oo, f);
        }
    }

    if first {
        let _ = writeln!(out, "            gpb_rd_skip(&mut r, wt);");
    } else {
        let _ = writeln!(out, "            }} else {{ gpb_rd_skip(&mut r, wt); }}");
    }
    let _ = writeln!(out, "        }}");

    // Финализация срезов из векторов
    for f in &m.fields {
        if matches!(f.ty, FieldType::Map(..)) {
            let _ = writeln!(out, "        m.{} = make_slice(vec_{}.data, vec_{}.len);", f.name, f.name, f.name);
        } else if f.repeated {
            if matches!(f.ty, FieldType::String | FieldType::Bytes) {
                let _ = writeln!(out, "        m.{} = make_slice(vec_{}.data, vec_{}.len);", f.name, f.name, f.name);
            } else if is_message(file, &f.ty) {
                let sub = flat(&f.ty);
                let _ = writeln!(out, "        m.{} = make_slice(vec_{}.data as *mut *{}, vec_{}.len);", f.name, f.name, sub, f.name);
            }
        }
    }

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

fn gen_decode_oneof_field(out: &mut String, file: &FileD, oo: &OneofD, f: &FieldD) {
    let name = &f.name;
    if scalar_gtype(file, &f.ty).is_some() {
        let (kind, conv) = scalar_read(file, &f.ty);
        let _ = writeln!(out, "                {}", read_stmt(&kind));
        let _ = writeln!(out, "                m.{name} = {conv};");
        let _ = writeln!(out, "                m.which_{} = {};", oo.name, f.number);
        return;
    }
    if matches!(f.ty, FieldType::String | FieldType::Bytes) {
        let _ = writeln!(out, "                let ln: i64 = gpb_rd_varint(&mut r) as i64;");
        let _ = writeln!(out, "                m.{name}_ptr = &r.data[r.pos];");
        let _ = writeln!(out, "                m.{name}_len = ln;");
        let _ = writeln!(out, "                r.pos = r.pos + ln;");
        let _ = writeln!(out, "                m.which_{} = {};", oo.name, f.number);
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
        let _ = writeln!(out, "                m.which_{} = {};", oo.name, f.number);
    }
}

fn gen_decode_map(
    out: &mut String,
    file: &FileD,
    msg_name: &str,
    f: &FieldD,
    k: &FieldType,
    v: &FieldType,
) {
    let entry_name = map_entry_name(msg_name, &f.name);
    let vec_fn = map_vec_fn(msg_name, &f.name);
    let name = &f.name;

    let _ = writeln!(out, "                let entry_len_{name}: i64 = gpb_rd_varint(&mut r) as i64;");
    let _ = writeln!(out, "                let mut entry_r_{name}: GpbReader = gpb_rd_new(&r.data[r.pos], entry_len_{name});");
    let _ = writeln!(out, "                let mut entry_item_{name}: {entry_name} = zeroed();");
    let _ = writeln!(out, "                for {{");
    let _ = writeln!(out, "                    if gpb_rd_eof(&mut entry_r_{name}) {{ break; }}");
    let _ = writeln!(out, "                    let entry_tag: u64 = gpb_rd_varint(&mut entry_r_{name});");
    let _ = writeln!(out, "                    let entry_fnum: u64 = entry_tag >> 3;");
    let _ = writeln!(out, "                    let entry_wt: u64 = entry_tag & 7;");

    // Key reading (field 1)
    let _ = writeln!(out, "                    if entry_fnum == 1 {{");
    if scalar_gtype(file, k).is_some() {
        let (kind, conv) = scalar_read(file, k);
        let _ = writeln!(out, "                        {}", read_stmt_with_reader(&format!("entry_r_{name}"), &kind));
        let _ = writeln!(out, "                        entry_item_{name}.key = {conv};");
    } else if matches!(k, FieldType::String | FieldType::Bytes) {
        let _ = writeln!(out, "                        let ln_k: i64 = gpb_rd_varint(&mut entry_r_{name}) as i64;");
        let _ = writeln!(out, "                        entry_item_{name}.key = GpbString {{ data: &entry_r_{name}.data[entry_r_{name}.pos], len: ln_k }};");
        let _ = writeln!(out, "                        entry_r_{name}.pos = entry_r_{name}.pos + ln_k;");
    }

    // Value reading (field 2)
    let _ = writeln!(out, "                    }} else if entry_fnum == 2 {{");
    if scalar_gtype(file, v).is_some() {
        let (kind, conv) = scalar_read(file, v);
        let _ = writeln!(out, "                        {}", read_stmt_with_reader(&format!("entry_r_{name}"), &kind));
        let _ = writeln!(out, "                        entry_item_{name}.value = {conv};");
    } else if matches!(v, FieldType::String | FieldType::Bytes) {
        let _ = writeln!(out, "                        let ln_v: i64 = gpb_rd_varint(&mut entry_r_{name}) as i64;");
        let _ = writeln!(out, "                        entry_item_{name}.value = GpbString {{ data: &entry_r_{name}.data[entry_r_{name}.pos], len: ln_v }};");
        let _ = writeln!(out, "                        entry_r_{name}.pos = entry_r_{name}.pos + ln_v;");
    } else if is_message(file, v) {
        let sub = flat(v);
        let _ = writeln!(out, "                        let ln_v: i64 = gpb_rd_varint(&mut entry_r_{name}) as i64;");
        let _ = writeln!(out, "                        let subp_v: *u8 = &entry_r_{name}.data[entry_r_{name}.pos];");
        let _ = writeln!(out, "                        let sp_v: *mut {sub} = alloc(sizeof({sub})) as *mut {sub};");
        let _ = writeln!(out, "                        *sp_v = decode_{sub}(subp_v, ln_v);");
        let _ = writeln!(out, "                        entry_item_{name}.value = sp_v;");
        let _ = writeln!(out, "                        entry_r_{name}.pos = entry_r_{name}.pos + ln_v;");
    }

    let _ = writeln!(out, "                    }} else {{ gpb_rd_skip(&mut entry_r_{name}, entry_wt); }}");
    let _ = writeln!(out, "                }}");
    let _ = writeln!(out, "                {vec_fn}_push(&mut vec_{name}, entry_item_{name});");
    let _ = writeln!(out, "                r.pos = r.pos + entry_len_{name};");
}

/// Декодирование packed repeated числовых полей.
fn gen_decode_packed(out: &mut String, file: &FileD, f: &FieldD) {
    let name = &f.name;
    let et = scalar_gtype(file, &f.ty).unwrap();
    let (kind, conv) = scalar_read(file, &f.ty);

    let _ = writeln!(out, "                let plen: i64 = gpb_rd_varint(&mut r) as i64;");
    let _ = writeln!(out, "                let start_{name}: i64 = r.pos;");
    let _ = writeln!(out, "                let end_{name}: i64 = start_{name} + plen;");

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
        if let FieldType::Map(..) = &f.ty {
            let entry_ty = map_entry_name(&m.name, &f.name);
            let _ = writeln!(out, "    {}: []{},", f.name, entry_ty);
            continue;
        }
        if f.repeated {
            if let Some(g) = scalar_gtype(file, &f.ty) {
                let _ = writeln!(out, "    {}: []{},", f.name, g);
            } else if matches!(f.ty, FieldType::String | FieldType::Bytes) {
                let _ = writeln!(out, "    {}: []GpbString,", f.name);
            } else if is_message(file, &f.ty) {
                let _ = writeln!(out, "    {}: []*{},", f.name, flat(&f.ty));
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
    for oo in &m.oneofs {
        let _ = writeln!(out, "    which_{}: i32,", oo.name);
        for f in &oo.fields {
            if let Some(g) = scalar_gtype(file, &f.ty) {
                let _ = writeln!(out, "    {}: {},", f.name, g);
            } else if matches!(f.ty, FieldType::String | FieldType::Bytes) {
                let _ = writeln!(out, "    {}_ptr: *u8,", f.name);
                let _ = writeln!(out, "    {}_len: i64,", f.name);
            } else if is_message(file, &f.ty) {
                let _ = writeln!(out, "    {}: *{},", f.name, flat(&f.ty));
            }
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
        if let FieldType::Map(k, v) = &f.ty {
            gen_map_encode(out, file, &m.name, f, k, v);
            continue;
        }
        if f.repeated {
            gen_repeated(out, file, f);
        } else {
            gen_singular(out, file, f);
        }
    }
    for oo in &m.oneofs {
        let _ = writeln!(out, "        // oneof {}", oo.name);
        for f in &oo.fields {
            let n = f.number as u64;
            let _ = writeln!(out, "        if m.which_{} == {} {{", oo.name, n);
            let base = format!("m.{}", f.name);
            let enum_field = is_enum(file, &f.ty);
            if scalar_gtype(file, &f.ty).is_some() {
                let (wt, valexpr) = scalar_wire(&f.ty, enum_field, &base);
                let _ = writeln!(out, "            gpb_tag(b, {n}, {wt}); {};", write_call(wt, &valexpr));
            } else if matches!(f.ty, FieldType::String | FieldType::Bytes) {
                let _ = writeln!(
                    out,
                    "            gpb_tag(b, {n}, {WT_LEN}); gpb_bytes(b, m.{name}_ptr, m.{name}_len);",
                    name = f.name
                );
            } else if is_message(file, &f.ty) {
                let sub = flat(&f.ty);
                let name = &f.name;
                let _ = writeln!(out, "            if (m.{name} as i64) != 0 {{");
                let _ = writeln!(out, "                let mut sub_{name} = gpb_buf_new();");
                let _ = writeln!(out, "                encode_{sub}(m.{name}, &mut sub_{name});");
                let _ = writeln!(out, "                gpb_tag(b, {n}, {WT_LEN});");
                let _ = writeln!(out, "                gpb_bytes(b, sub_{name}.data as *u8, sub_{name}.len);");
                let _ = writeln!(out, "                free(sub_{name}.data);");
                let _ = writeln!(out, "            }}");
            }
            let _ = writeln!(out, "        }}");
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

fn gen_map_encode(
    out: &mut String,
    file: &FileD,
    _msg_name: &str,
    f: &FieldD,
    k: &FieldType,
    v: &FieldType,
) {
    let n = f.number as u64;
    let name = &f.name;
    let _ = writeln!(out, "        for i_{name} := 0; i_{name} < m.{name}.len; i_{name}++ {{");
    let _ = writeln!(out, "            let mut entry_buf_{name} = gpb_buf_new();");

    // Key (field 1)
    let k_base = format!("m.{name}[i_{name}].key");
    if scalar_gtype(file, k).is_some() {
        let (wt, valexpr) = scalar_wire(k, is_enum(file, k), &k_base);
        let _ = writeln!(
            out,
            "            gpb_tag(&mut entry_buf_{name}, 1, {wt}); {}",
            write_call_to(wt, &format!("&mut entry_buf_{name}"), &valexpr)
        );
    } else if matches!(k, FieldType::String | FieldType::Bytes) {
        let _ = writeln!(
            out,
            "            gpb_tag(&mut entry_buf_{name}, 1, {WT_LEN}); gpb_bytes(&mut entry_buf_{name}, {k_base}.data, {k_base}.len);"
        );
    }

    // Value (field 2)
    let v_base = format!("m.{name}[i_{name}].value");
    if scalar_gtype(file, v).is_some() {
        let (wt, valexpr) = scalar_wire(v, is_enum(file, v), &v_base);
        let _ = writeln!(
            out,
            "            gpb_tag(&mut entry_buf_{name}, 2, {wt}); {}",
            write_call_to(wt, &format!("&mut entry_buf_{name}"), &valexpr)
        );
    } else if matches!(v, FieldType::String | FieldType::Bytes) {
        let _ = writeln!(
            out,
            "            gpb_tag(&mut entry_buf_{name}, 2, {WT_LEN}); gpb_bytes(&mut entry_buf_{name}, {v_base}.data, {v_base}.len);"
        );
    } else if is_message(file, v) {
        let sub = flat(v);
        let _ = writeln!(out, "            if ({v_base} as i64) != 0 {{");
        let _ = writeln!(out, "                let mut sub_val_buf = gpb_buf_new();");
        let _ = writeln!(out, "                encode_{sub}({v_base}, &mut sub_val_buf);");
        let _ = writeln!(out, "                gpb_tag(&mut entry_buf_{name}, 2, {WT_LEN});");
        let _ = writeln!(
            out,
            "                gpb_bytes(&mut entry_buf_{name}, sub_val_buf.data as *u8, sub_val_buf.len);"
        );
        let _ = writeln!(out, "                free(sub_val_buf.data);");
        let _ = writeln!(out, "            }}");
    }

    let _ = writeln!(out, "            gpb_tag(b, {n}, {WT_LEN});");
    let _ = writeln!(
        out,
        "            gpb_bytes(b, entry_buf_{name}.data as *u8, entry_buf_{name}.len);"
    );
    let _ = writeln!(out, "            free(entry_buf_{name}.data);");
    let _ = writeln!(out, "        }}");
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
    // repeated string / bytes — всегда expanded, каждый элемент length-delimited.
    if matches!(f.ty, FieldType::String | FieldType::Bytes) {
        let _ = writeln!(out, "        for i_{name} := 0; i_{name} < m.{name}.len; i_{name}++ {{");
        let _ = writeln!(out, "            gpb_tag(b, {n}, {WT_LEN});");
        let _ = writeln!(out, "            gpb_bytes(b, m.{name}[i_{name}].data, m.{name}[i_{name}].len);");
        let _ = writeln!(out, "        }}");
        return;
    }
    // repeated скаляр.
    if scalar_gtype(file, &f.ty).is_none() {
        return;
    }
    let elem = format!("m.{name}[i_{name}]");
    let (wt, valexpr) = scalar_wire(&f.ty, enum_field, &elem);

    if f.packed {
        // packed: длина-делимитированная упаковка значений.
        let _ = writeln!(out, "        if m.{name}.len != 0 {{");
        let _ = writeln!(out, "            let mut pk_{name} = gpb_buf_new();");
        let _ = writeln!(out, "            for i_{name} := 0; i_{name} < m.{name}.len; i_{name}++ {{");
        let _ = writeln!(
            out,
            "                {}",
            write_call_to(wt, &format!("&mut pk_{name}"), &valexpr).replace("{name}", name)
        );
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
