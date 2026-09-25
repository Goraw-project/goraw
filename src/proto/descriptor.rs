//! Разрешённая модель дескрипторов + резолвинг Editions-features.
//! Именно резолвинг features (дефолты редакции + наследование
//! file → message → field) — Editions-специфичная соль.

use crate::proto::ast::*;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Presence {
    Explicit,      // есть hasbit (proto2-подобно) — дефолт edition 2023
    Implicit,      // нет hasbit, пишем если не нулевое (proto3-подобно)
    LegacyRequired,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RepeatedEnc {
    Packed,
    Expanded,
}

#[derive(Clone, Copy, Debug)]
pub struct Features {
    pub presence: Presence,
    pub repeated: RepeatedEnc,
    pub enum_open: bool,
    pub delimited: bool, // message_encoding = DELIMITED
}

impl Features {
    /// Дефолты для конкретной редакции.
    pub fn defaults_for(edition: &str) -> Features {
        match edition {
            "proto2" => Features { presence: Presence::Explicit, repeated: RepeatedEnc::Expanded, enum_open: false, delimited: false },
            "proto3" => Features { presence: Presence::Implicit, repeated: RepeatedEnc::Packed, enum_open: true, delimited: false },
            // edition 2023 (и как безопасный дефолт для будущих редакций)
            _ => Features { presence: Presence::Explicit, repeated: RepeatedEnc::Packed, enum_open: true, delimited: false },
        }
    }

    /// Применяет `features.*` из списка опций поверх текущего набора.
    pub fn apply(&mut self, opts: &[Opt]) {
        for o in opts {
            let v = match &o.value {
                OptValue::Ident(s) => s.as_str(),
                OptValue::Bool(true) => "true",
                OptValue::Bool(false) => "false",
                _ => continue,
            };
            match o.path.as_str() {
                "features.field_presence" => {
                    self.presence = match v {
                        "IMPLICIT" => Presence::Implicit,
                        "LEGACY_REQUIRED" => Presence::LegacyRequired,
                        _ => Presence::Explicit,
                    };
                }
                "features.repeated_field_encoding" => {
                    self.repeated = if v == "EXPANDED" { RepeatedEnc::Expanded } else { RepeatedEnc::Packed };
                }
                "features.enum_type" => {
                    self.enum_open = v != "CLOSED";
                }
                "features.message_encoding" => {
                    self.delimited = v == "DELIMITED";
                }
                // legacy [packed = true/false]
                "packed" => {
                    self.repeated = if v == "false" { RepeatedEnc::Expanded } else { RepeatedEnc::Packed };
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct FieldD {
    pub name: String,
    pub number: i64,
    pub ty: FieldType,
    pub repeated: bool,
    pub presence: Presence,
    pub packed: bool, // эффективно ли packed-кодирование (repeated + пакуемый скаляр)
}

#[derive(Clone, Debug)]
pub struct OneofD {
    pub name: String,
    pub fields: Vec<FieldD>,
}

#[derive(Clone, Debug)]
pub struct MessageD {
    pub name: String, // сплющенное имя (Outer_Inner)
    pub fields: Vec<FieldD>,
    pub oneofs: Vec<OneofD>,
}

pub struct FileD {
    pub edition: String,
    pub messages: Vec<MessageD>,
    pub message_names: HashSet<String>, // простые имена сообщений
    pub enum_names: HashSet<String>,    // простые имена enum
}

/// Строит разрешённую модель из AST: сплющивает вложенные типы и резолвит
/// features с наследованием.
pub fn resolve(file: &ProtoFile) -> FileD {
    let edition = if file.edition.is_empty() { "2023".to_string() } else { file.edition.clone() };
    let file_feat = {
        let mut f = Features::defaults_for(&edition);
        f.apply(&file.options);
        f
    };

    // Сбор простых имён сообщений/enum (для различения Named → message/enum).
    let mut message_names = HashSet::new();
    let mut enum_names = HashSet::new();
    collect_names(&file.messages, &file.enums, &mut message_names, &mut enum_names);

    let mut messages = Vec::new();
    for m in &file.messages {
        flatten_message(m, "", file_feat, &enum_names, &mut messages);
    }

    FileD { edition, messages, message_names, enum_names }
}

fn collect_names(
    msgs: &[Message],
    enums: &[EnumDef],
    mnames: &mut HashSet<String>,
    enames: &mut HashSet<String>,
) {
    for e in enums {
        enames.insert(e.name.clone());
    }
    for m in msgs {
        mnames.insert(m.name.clone());
        collect_names(&m.messages, &m.enums, mnames, enames);
    }
}

fn flatten_message(
    m: &Message,
    prefix: &str,
    parent_feat: Features,
    enum_names: &HashSet<String>,
    out: &mut Vec<MessageD>,
) {
    let flat = if prefix.is_empty() { m.name.clone() } else { format!("{prefix}_{}", m.name) };

    // features сообщения наследуют родительские и применяют свои.
    let mut msg_feat = parent_feat;
    msg_feat.apply(&m.options);

    let mut fields = Vec::new();
    // обычные поля
    for f in &m.fields {
        fields.push(resolve_field(f, msg_feat, enum_names));
    }
    // поля из oneof
    let mut oneofs = Vec::new();
    for oo in &m.oneofs {
        let mut oo_fields = Vec::new();
        for f in &oo.fields {
            let mut fd = resolve_field(f, msg_feat, enum_names);
            fd.presence = Presence::Explicit;
            oo_fields.push(fd);
        }
        oneofs.push(OneofD {
            name: oo.name.clone(),
            fields: oo_fields,
        });
    }

    out.push(MessageD { name: flat.clone(), fields, oneofs });

    // вложенные сообщения
    for nested in &m.messages {
        flatten_message(nested, &flat, msg_feat, enum_names, out);
    }
}

fn resolve_field(f: &Field, msg_feat: Features, enum_names: &HashSet<String>) -> FieldD {
    let mut ff = msg_feat;
    ff.apply(&f.options);

    let is_enum = matches!(&f.ty, FieldType::Named(n) if enum_names.contains(simple_name(n)));
    let packable = is_packable(&f.ty, is_enum);
    let packed = f.repeated && packable && ff.repeated == RepeatedEnc::Packed;

    FieldD {
        name: f.name.clone(),
        number: f.number,
        ty: f.ty.clone(),
        repeated: f.repeated,
        presence: ff.presence,
        packed,
    }
}

fn simple_name(n: &str) -> &str {
    n.rsplit('.').next().unwrap_or(n)
}

/// Пакуемы ли значения этого типа (числовые/bool/enum). Строки/bytes/
/// сообщения — никогда.
fn is_packable(ty: &FieldType, is_enum: bool) -> bool {
    use FieldType::*;
    if is_enum {
        return true;
    }
    matches!(
        ty,
        Int32 | Int64 | UInt32 | UInt64 | SInt32 | SInt64 | Fixed32 | Fixed64 | SFixed32
            | SFixed64 | Bool | Float | Double
    )
}
