//! AST схемы `.proto` (Protobuf **Editions**). Clean-room: собственные
//! структуры, не заимствованные из descriptor.proto Google.

use crate::diag::Span;

#[derive(Clone, Debug)]
pub struct ProtoFile {
    pub edition: String,           // напр. "2023"
    pub package: Option<String>,
    pub imports: Vec<String>,
    pub options: Vec<Opt>,         // file-level, включая features.*
    pub messages: Vec<Message>,
    pub enums: Vec<EnumDef>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub name: String,
    pub fields: Vec<Field>,
    pub oneofs: Vec<Oneof>,
    pub messages: Vec<Message>,    // вложенные
    pub enums: Vec<EnumDef>,
    pub options: Vec<Opt>,         // message-level features
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Oneof {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub number: i64,
    pub ty: FieldType,
    pub repeated: bool,
    pub options: Vec<Opt>,         // field-level, включая features.*
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldType {
    Int32,
    Int64,
    UInt32,
    UInt64,
    SInt32,
    SInt64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
    Bool,
    Float,
    Double,
    String,
    Bytes,
    /// Ссылка по имени (сообщение или enum — разрешается позже).
    Named(String),
    /// map<K, V>.
    Map(Box<FieldType>, Box<FieldType>),
}

#[derive(Clone, Debug)]
pub struct EnumDef {
    pub name: String,
    pub values: Vec<(String, i64)>,
    pub options: Vec<Opt>,
    pub span: Span,
}

/// Опция вида `имя.путь = значение` (в т.ч. `features.field_presence = ...`).
#[derive(Clone, Debug)]
pub struct Opt {
    pub path: String,
    pub value: OptValue,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum OptValue {
    Bool(bool),
    Int(i64),
    Str(String),
    Ident(String),
}
