//! Лексер + парсер `.proto` (Protobuf Editions), подмножество достаточное
//! для практических схем: edition/package/import/option, message (поля,
//! oneof, вложенные message/enum, map), enum. Ошибки — через `crate::diag`
//! (те же LLM-JSON диагностики).

use crate::diag::{Diagnostic, Diags, Pos, Span};
use crate::proto::ast::*;

#[derive(Clone, Debug, PartialEq)]
enum Tk {
    Ident(String),
    Int(i64),
    Str(String),
    Punct(char), // { } [ ] ( ) < > = ; , .
    Eof,
}

#[derive(Clone)]
struct Token {
    tk: Tk,
    span: Span,
}

struct Lexer {
    chars: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
}

impl Lexer {
    fn new(src: &str) -> Lexer {
        Lexer { chars: src.chars().collect(), i: 0, line: 1, col: 1 }
    }
    fn pos(&self) -> Pos {
        Pos { offset: self.i, line: self.line, col: self.col }
    }
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }
    fn peek2(&self) -> Option<char> {
        self.chars.get(self.i + 1).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.i).copied()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn tokenize(&mut self, diags: &mut Diags) -> Vec<Token> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia();
            let start = self.pos();
            let c = match self.peek() {
                Some(c) => c,
                None => {
                    out.push(Token { tk: Tk::Eof, span: Span::new(start, start) });
                    return out;
                }
            };
            if c.is_ascii_digit() || (c == '-' && self.peek2().map(|d| d.is_ascii_digit()).unwrap_or(false)) {
                out.push(self.lex_number());
            } else if c == '_' || c.is_alphabetic() {
                out.push(self.lex_ident());
            } else if c == '"' || c == '\'' {
                out.push(self.lex_string());
            } else {
                self.bump();
                match c {
                    '{' | '}' | '[' | ']' | '(' | ')' | '<' | '>' | '=' | ';' | ',' | '.' => {
                        out.push(Token { tk: Tk::Punct(c), span: Span::new(start, self.pos()) });
                    }
                    other => {
                        diags.push(Diagnostic::error(
                            "P0001",
                            Span::new(start, self.pos()),
                            format!("неизвестный символ `{other}` в .proto"),
                        ));
                    }
                }
            }
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek2() == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.peek2() == Some('*') => {
                    self.bump();
                    self.bump();
                    while let Some(c) = self.bump() {
                        if c == '*' && self.peek() == Some('/') {
                            self.bump();
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn lex_ident(&mut self) -> Token {
        let start = self.pos();
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == '_' || c.is_alphanumeric() {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        Token { tk: Tk::Ident(s), span: Span::new(start, self.pos()) }
    }

    fn lex_number(&mut self) -> Token {
        let start = self.pos();
        let mut s = String::new();
        if self.peek() == Some('-') {
            s.push('-');
            self.bump();
        }
        let mut radix = 10;
        if self.peek() == Some('0') && matches!(self.peek2(), Some('x') | Some('X')) {
            self.bump();
            self.bump();
            radix = 16;
        }
        while let Some(c) = self.peek() {
            if c.is_digit(radix) || c == '_' {
                if c != '_' {
                    s.push(c);
                }
                self.bump();
            } else {
                break;
            }
        }
        let val = if radix == 16 {
            i64::from_str_radix(s.trim_start_matches('-'), 16).map(|v| if s.starts_with('-') { -v } else { v }).unwrap_or(0)
        } else {
            s.parse::<i64>().unwrap_or(0)
        };
        Token { tk: Tk::Int(val), span: Span::new(start, self.pos()) }
    }

    fn lex_string(&mut self) -> Token {
        let start = self.pos();
        let quote = self.bump().unwrap();
        let mut s = String::new();
        while let Some(c) = self.bump() {
            if c == quote {
                break;
            }
            if c == '\\' {
                match self.bump() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some(o) => s.push(o),
                    None => break,
                }
            } else {
                s.push(c);
            }
        }
        Token { tk: Tk::Str(s), span: Span::new(start, self.pos()) }
    }
}

pub struct Parser<'a> {
    toks: Vec<Token>,
    i: usize,
    diags: &'a mut Diags,
}

type R<T> = Option<T>;

impl<'a> Parser<'a> {
    pub fn new(src: &str, diags: &'a mut Diags) -> Parser<'a> {
        let mut lx = Lexer::new(src);
        let toks = lx.tokenize(diags);
        Parser { toks, i: 0, diags }
    }

    fn peek(&self) -> &Tk {
        &self.toks[self.i].tk
    }
    fn span(&self) -> Span {
        self.toks[self.i].span
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.i].clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }
    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tk::Eof)
    }
    fn is_punct(&self, c: char) -> bool {
        matches!(self.peek(), Tk::Punct(p) if *p == c)
    }
    fn eat_punct(&mut self, c: char) -> bool {
        if self.is_punct(c) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tk::Ident(s) if s == kw)
    }

    fn expect_punct(&mut self, c: char) -> R<()> {
        if self.eat_punct(c) {
            Some(())
        } else {
            self.err(format!("ожидался `{c}`"));
            None
        }
    }
    fn expect_ident(&mut self) -> R<String> {
        if let Tk::Ident(s) = self.peek().clone() {
            self.bump();
            Some(s)
        } else {
            self.err("ожидалось имя".into());
            None
        }
    }
    fn err(&mut self, msg: String) {
        self.diags.push(Diagnostic::error("P0002", self.span(), msg));
    }

    /// Пропуск до `;` или до конца блока — восстановление.
    fn recover(&mut self) {
        while !self.at_eof() {
            if self.eat_punct(';') {
                return;
            }
            if self.is_punct('}') {
                return;
            }
            self.bump();
        }
    }

    pub fn parse_file(&mut self) -> ProtoFile {
        let start = self.span();
        let mut file = ProtoFile {
            edition: String::new(),
            package: None,
            imports: Vec::new(),
            options: Vec::new(),
            messages: Vec::new(),
            enums: Vec::new(),
            span: start,
        };

        while !self.at_eof() {
            if self.eat_punct(';') {
                continue;
            }
            if self.is_kw("edition") || self.is_kw("syntax") {
                self.bump();
                if self.expect_punct('=').is_some() {
                    if let Tk::Str(s) = self.peek().clone() {
                        self.bump();
                        file.edition = s;
                    } else {
                        self.err("ожидалась строка редакции".into());
                    }
                }
                let _ = self.expect_punct(';');
            } else if self.is_kw("package") {
                self.bump();
                file.package = self.parse_dotted();
                let _ = self.expect_punct(';');
            } else if self.is_kw("import") {
                self.bump();
                // возможные квалификаторы public/weak
                if self.is_kw("public") || self.is_kw("weak") {
                    self.bump();
                }
                if let Tk::Str(s) = self.peek().clone() {
                    self.bump();
                    file.imports.push(s);
                }
                let _ = self.expect_punct(';');
            } else if self.is_kw("option") {
                self.bump();
                if let Some(o) = self.parse_option_body() {
                    file.options.push(o);
                }
            } else if self.is_kw("message") {
                self.bump();
                match self.parse_message() {
                    Some(m) => file.messages.push(m),
                    None => self.recover(),
                }
            } else if self.is_kw("enum") {
                self.bump();
                match self.parse_enum() {
                    Some(e) => file.enums.push(e),
                    None => self.recover(),
                }
            } else if self.is_kw("service") {
                // сервисы парсим-и-пропускаем (PB3)
                self.bump();
                let _ = self.expect_ident();
                self.skip_block();
            } else {
                self.err(format!("неожиданный токен на верхнем уровне: {:?}", self.peek()));
                self.recover();
            }
        }
        file
    }

    fn parse_dotted(&mut self) -> Option<String> {
        let mut s = self.expect_ident()?;
        while self.eat_punct('.') {
            let part = self.expect_ident()?;
            s.push('.');
            s.push_str(&part);
        }
        Some(s)
    }

    /// `path = value ;` (после ключевого слова option или внутри `[ ]`).
    fn parse_option_body(&mut self) -> Option<Opt> {
        let span = self.span();
        // путь: dotted, возможно с (custom) — custom пропускаем
        let mut path = String::new();
        if self.eat_punct('(') {
            // (custom.option)
            while !self.is_punct(')') && !self.at_eof() {
                self.bump();
            }
            let _ = self.expect_punct(')');
            // возможный .suffix
            while self.eat_punct('.') {
                if let Some(p) = self.opt_ident() {
                    path.push('.');
                    path.push_str(&p);
                }
            }
        } else {
            path = self.parse_dotted()?;
        }
        self.expect_punct('=')?;
        let value = self.parse_value()?;
        let _ = self.expect_punct(';');
        Some(Opt { path, value, span })
    }

    fn opt_ident(&mut self) -> Option<String> {
        if let Tk::Ident(s) = self.peek().clone() {
            self.bump();
            Some(s)
        } else {
            None
        }
    }

    fn parse_value(&mut self) -> Option<OptValue> {
        match self.peek().clone() {
            Tk::Ident(s) => {
                self.bump();
                match s.as_str() {
                    "true" => Some(OptValue::Bool(true)),
                    "false" => Some(OptValue::Bool(false)),
                    _ => Some(OptValue::Ident(s)),
                }
            }
            Tk::Int(n) => {
                self.bump();
                Some(OptValue::Int(n))
            }
            Tk::Str(s) => {
                self.bump();
                Some(OptValue::Str(s))
            }
            _ => {
                self.err("ожидалось значение опции".into());
                None
            }
        }
    }

    fn parse_message(&mut self) -> Option<Message> {
        let span = self.span();
        let name = self.expect_ident()?;
        self.expect_punct('{')?;
        let mut m = Message {
            name,
            fields: Vec::new(),
            oneofs: Vec::new(),
            messages: Vec::new(),
            enums: Vec::new(),
            options: Vec::new(),
            span,
        };
        while !self.is_punct('}') && !self.at_eof() {
            if self.eat_punct(';') {
                continue;
            }
            if self.is_kw("message") {
                self.bump();
                if let Some(nested) = self.parse_message() {
                    m.messages.push(nested);
                } else {
                    self.recover();
                }
            } else if self.is_kw("enum") {
                self.bump();
                if let Some(e) = self.parse_enum() {
                    m.enums.push(e);
                } else {
                    self.recover();
                }
            } else if self.is_kw("option") {
                self.bump();
                if let Some(o) = self.parse_option_body() {
                    m.options.push(o);
                }
            } else if self.is_kw("oneof") {
                self.bump();
                if let Some(o) = self.parse_oneof() {
                    m.oneofs.push(o);
                } else {
                    self.recover();
                }
            } else if self.is_kw("reserved") || self.is_kw("extensions") {
                // пропускаем до ;
                while !self.eat_punct(';') && !self.at_eof() && !self.is_punct('}') {
                    self.bump();
                }
            } else {
                match self.parse_field() {
                    Some(f) => m.fields.push(f),
                    None => self.recover(),
                }
            }
        }
        self.expect_punct('}')?;
        Some(m)
    }

    fn parse_oneof(&mut self) -> Option<Oneof> {
        let span = self.span();
        let name = self.expect_ident()?;
        self.expect_punct('{')?;
        let mut fields = Vec::new();
        while !self.is_punct('}') && !self.at_eof() {
            if self.eat_punct(';') {
                continue;
            }
            match self.parse_field() {
                Some(f) => fields.push(f),
                None => self.recover(),
            }
        }
        self.expect_punct('}')?;
        Some(Oneof { name, fields, span })
    }

    fn parse_field(&mut self) -> Option<Field> {
        let span = self.span();
        let mut repeated = false;
        if self.is_kw("repeated") {
            self.bump();
            repeated = true;
        } else if self.is_kw("optional") || self.is_kw("required") {
            // в Editions этих меток нет, но терпимо принимаем
            self.bump();
        }
        let ty = self.parse_field_type()?;
        let name = self.expect_ident()?;
        self.expect_punct('=')?;
        let number = match self.peek().clone() {
            Tk::Int(n) => {
                self.bump();
                n
            }
            _ => {
                self.err("ожидался номер поля".into());
                return None;
            }
        };
        let options = self.parse_field_options();
        let _ = self.expect_punct(';');
        Some(Field { name, number, ty, repeated, options, span })
    }

    fn parse_field_type(&mut self) -> Option<FieldType> {
        if self.is_kw("map") {
            self.bump();
            self.expect_punct('<')?;
            let k = self.parse_field_type()?;
            self.expect_punct(',')?;
            let v = self.parse_field_type()?;
            self.expect_punct('>')?;
            return Some(FieldType::Map(Box::new(k), Box::new(v)));
        }
        let name = self.parse_dotted()?;
        Some(match name.as_str() {
            "int32" => FieldType::Int32,
            "int64" => FieldType::Int64,
            "uint32" => FieldType::UInt32,
            "uint64" => FieldType::UInt64,
            "sint32" => FieldType::SInt32,
            "sint64" => FieldType::SInt64,
            "fixed32" => FieldType::Fixed32,
            "fixed64" => FieldType::Fixed64,
            "sfixed32" => FieldType::SFixed32,
            "sfixed64" => FieldType::SFixed64,
            "bool" => FieldType::Bool,
            "float" => FieldType::Float,
            "double" => FieldType::Double,
            "string" => FieldType::String,
            "bytes" => FieldType::Bytes,
            _ => FieldType::Named(name),
        })
    }

    fn parse_field_options(&mut self) -> Vec<Opt> {
        let mut opts = Vec::new();
        if self.eat_punct('[') {
            while !self.is_punct(']') && !self.at_eof() {
                let span = self.span();
                // path (dotted, возможно (custom))
                let path = if self.is_punct('(') {
                    self.bump();
                    while !self.is_punct(')') && !self.at_eof() {
                        self.bump();
                    }
                    let _ = self.expect_punct(')');
                    let mut p = String::new();
                    while self.eat_punct('.') {
                        if let Some(x) = self.opt_ident() {
                            p.push('.');
                            p.push_str(&x);
                        }
                    }
                    p
                } else {
                    self.parse_dotted().unwrap_or_default()
                };
                if self.eat_punct('=') {
                    if let Some(value) = self.parse_value() {
                        opts.push(Opt { path, value, span });
                    }
                }
                if !self.eat_punct(',') {
                    break;
                }
            }
            let _ = self.expect_punct(']');
        }
        opts
    }

    fn parse_enum(&mut self) -> Option<EnumDef> {
        let span = self.span();
        let name = self.expect_ident()?;
        self.expect_punct('{')?;
        let mut values = Vec::new();
        let mut options = Vec::new();
        while !self.is_punct('}') && !self.at_eof() {
            if self.eat_punct(';') {
                continue;
            }
            if self.is_kw("option") {
                self.bump();
                if let Some(o) = self.parse_option_body() {
                    options.push(o);
                }
                continue;
            }
            if self.is_kw("reserved") {
                while !self.eat_punct(';') && !self.at_eof() && !self.is_punct('}') {
                    self.bump();
                }
                continue;
            }
            let vname = self.expect_ident()?;
            self.expect_punct('=')?;
            let num = match self.peek().clone() {
                Tk::Int(n) => {
                    self.bump();
                    n
                }
                _ => {
                    self.err("ожидалось число enum".into());
                    return None;
                }
            };
            let _ = self.parse_field_options();
            let _ = self.expect_punct(';');
            values.push((vname, num));
        }
        self.expect_punct('}')?;
        Some(EnumDef { name, values, options, span })
    }

    fn skip_block(&mut self) {
        // пропускает `{ ... }` с балансировкой
        if self.eat_punct('{') {
            let mut depth = 1;
            while depth > 0 && !self.at_eof() {
                if self.is_punct('{') {
                    depth += 1;
                } else if self.is_punct('}') {
                    depth -= 1;
                }
                self.bump();
            }
        }
    }
}
