//! Лексер Goraw. Превращает исходный текст в поток токенов со спанами.
//! Ошибки лексики (незакрытая строка, левый символ) складываются в `Diags`,
//! лексер при этом старается продолжить — чтобы за один проход собрать
//! максимум проблем.

use crate::diag::{Diagnostic, Diags, Pos, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    // литералы / идентификаторы
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),

    // ключевые слова
    Fn,
    Extern,
    Let,
    Mut,
    Return,
    If,
    Else,
    While,
    For,
    Struct,
    Unsafe,
    As,
    True,
    False,
    Break,
    Continue,
    Asm,
    Jit,
    Null,

    // пунктуация / операторы
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semi,
    Dot,
    Arrow,     // ->
    Ellipsis,  // ...
    ColonEq,   // :=  (Go-style)
    Assign,    // =
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,       // &
    Pipe,      // |
    Caret,     // ^
    Shl,       // <<
    Shr,       // >>
    Bang,      // !
    AndAnd,    // &&
    OrOr,      // ||
    PipeArrow, // |>  (конвейер)
    EqEq,      // ==
    Ne,        // !=
    Lt,
    Le,
    Gt,
    Ge,
    PlusPlus,   // ++
    MinusMinus, // --

    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    chars: Vec<char>, // исходник в char'ах для корректной работы с не-ASCII
    // соответствие char-индекса -> байтовое смещение
    byte_at: Vec<usize>,
    i: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Lexer<'a> {
        let chars: Vec<char> = src.chars().collect();
        let mut byte_at = Vec::with_capacity(chars.len() + 1);
        let mut b = 0usize;
        for c in &chars {
            byte_at.push(b);
            b += c.len_utf8();
        }
        byte_at.push(b);
        Lexer { src: src.as_bytes(), chars, byte_at, i: 0, line: 1, col: 1 }
    }

    fn pos(&self) -> Pos {
        Pos { offset: self.byte_at[self.i.min(self.byte_at.len() - 1)], line: self.line, col: self.col }
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

    pub fn tokenize(&mut self, diags: &mut Diags) -> Vec<Token> {
        let _ = self.src; // src хранится для симметрии, работа идёт по chars
        let mut out = Vec::new();
        loop {
            self.skip_trivia(diags);
            let start = self.pos();
            let c = match self.peek() {
                Some(c) => c,
                None => {
                    out.push(Token { tok: Tok::Eof, span: Span::new(start, start) });
                    break;
                }
            };

            if c.is_ascii_digit() {
                out.push(self.lex_number(diags));
                continue;
            }
            if c == '_' || c.is_alphabetic() {
                out.push(self.lex_ident());
                continue;
            }
            if c == '"' {
                out.push(self.lex_string(diags));
                continue;
            }

            // операторы и пунктуация
            let tok = self.lex_operator(diags);
            let end = self.pos();
            if let Some(t) = tok {
                out.push(Token { tok: t, span: Span::new(start, end) });
            }
        }
        out
    }

    fn skip_trivia(&mut self, diags: &mut Diags) {
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
                    let start = self.pos();
                    self.bump();
                    self.bump();
                    let mut depth = 1;
                    while depth > 0 {
                        match self.bump() {
                            None => {
                                diags.push(
                                    Diagnostic::error(
                                        "E0002",
                                        Span::new(start, self.pos()),
                                        "незакрытый блочный комментарий",
                                    )
                                    .with_hint("добавьте `*/` в конце комментария"),
                                );
                                return;
                            }
                            Some('*') if self.peek() == Some('/') => {
                                self.bump();
                                depth -= 1;
                            }
                            Some('/') if self.peek() == Some('*') => {
                                self.bump();
                                depth += 1;
                            }
                            _ => {}
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
        let end = self.pos();
        let tok = match s.as_str() {
            "fn" => Tok::Fn,
            "extern" => Tok::Extern,
            "let" => Tok::Let,
            "mut" => Tok::Mut,
            "return" => Tok::Return,
            "if" => Tok::If,
            "else" => Tok::Else,
            "while" => Tok::While,
            "for" => Tok::For,
            "struct" => Tok::Struct,
            "unsafe" => Tok::Unsafe,
            "as" => Tok::As,
            "true" => Tok::True,
            "false" => Tok::False,
            "break" => Tok::Break,
            "continue" => Tok::Continue,
            "asm" => Tok::Asm,
            "jit" => Tok::Jit,
            "null" => Tok::Null,
            _ => Tok::Ident(s),
        };
        Token { tok, span: Span::new(start, end) }
    }

    fn lex_number(&mut self, diags: &mut Diags) -> Token {
        let start = self.pos();
        let mut s = String::new();

        // hex / bin / oct
        if self.peek() == Some('0') {
            if let Some(p) = self.peek2() {
                if p == 'x' || p == 'X' || p == 'b' || p == 'B' || p == 'o' || p == 'O' {
                    self.bump();
                    let radix_c = self.bump().unwrap();
                    let radix = match radix_c {
                        'x' | 'X' => 16,
                        'b' | 'B' => 2,
                        _ => 8,
                    };
                    let mut digits = String::new();
                    while let Some(c) = self.peek() {
                        if c == '_' {
                            self.bump();
                            continue;
                        }
                        if c.is_digit(radix) {
                            digits.push(c);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let end = self.pos();
                    let val = i64::from_str_radix(&digits, radix).unwrap_or(0);
                    return Token { tok: Tok::Int(val), span: Span::new(start, end) };
                }
            }
        }

        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.bump();
            } else if c == '_' {
                self.bump();
            } else if c == '.' && self.peek2().map(|d| d.is_ascii_digit()).unwrap_or(false) && !is_float {
                is_float = true;
                s.push('.');
                self.bump();
            } else if (c == 'e' || c == 'E') && !s.is_empty() {
                is_float = true;
                s.push('e');
                self.bump();
                if let Some(sign) = self.peek() {
                    if sign == '+' || sign == '-' {
                        s.push(sign);
                        self.bump();
                    }
                }
            } else {
                break;
            }
        }
        let end = self.pos();
        let span = Span::new(start, end);
        if is_float {
            match s.parse::<f64>() {
                Ok(f) => Token { tok: Tok::Float(f), span },
                Err(_) => {
                    diags.push(Diagnostic::error("E0003", span, format!("нераспознанный float-литерал `{s}`")));
                    Token { tok: Tok::Float(0.0), span }
                }
            }
        } else {
            match s.parse::<i64>() {
                Ok(n) => Token { tok: Tok::Int(n), span },
                Err(_) => {
                    diags.push(Diagnostic::error("E0003", span, format!("целочисленный литерал `{s}` не помещается в i64")));
                    Token { tok: Tok::Int(0), span }
                }
            }
        }
    }

    fn lex_string(&mut self, diags: &mut Diags) -> Token {
        let start = self.pos();
        self.bump(); // "
        let mut s = String::new();
        loop {
            match self.bump() {
                None => {
                    diags.push(
                        Diagnostic::error("E0004", Span::new(start, self.pos()), "незакрытый строковый литерал")
                            .with_hint("добавьте закрывающую кавычку `\"`"),
                    );
                    break;
                }
                Some('"') => break,
                Some('\\') => match self.bump() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('0') => s.push('\0'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some('\'') => s.push('\''),
                    Some(other) => {
                        diags.push(Diagnostic::error(
                            "E0005",
                            Span::new(start, self.pos()),
                            format!("неизвестная escape-последовательность `\\{other}`"),
                        ));
                        s.push(other);
                    }
                    None => {}
                },
                Some(c) => s.push(c),
            }
        }
        let end = self.pos();
        Token { tok: Tok::Str(s), span: Span::new(start, end) }
    }

    fn lex_operator(&mut self, diags: &mut Diags) -> Option<Tok> {
        let start = self.pos();
        let c = self.bump().unwrap();
        let t = match c {
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            ',' => Tok::Comma,
            ';' => Tok::Semi,
            '.' => {
                if self.peek() == Some('.') && self.peek2() == Some('.') {
                    self.bump();
                    self.bump();
                    Tok::Ellipsis
                } else {
                    Tok::Dot
                }
            }
            ':' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Tok::ColonEq
                } else {
                    Tok::Colon
                }
            }
            '-' => match self.peek() {
                Some('>') => {
                    self.bump();
                    Tok::Arrow
                }
                Some('-') => {
                    self.bump();
                    Tok::MinusMinus
                }
                _ => Tok::Minus,
            },
            '+' => {
                if self.peek() == Some('+') {
                    self.bump();
                    Tok::PlusPlus
                } else {
                    Tok::Plus
                }
            }
            '*' => Tok::Star,
            '/' => Tok::Slash,
            '%' => Tok::Percent,
            '^' => Tok::Caret,
            '&' => {
                if self.peek() == Some('&') {
                    self.bump();
                    Tok::AndAnd
                } else {
                    Tok::Amp
                }
            }
            '|' => match self.peek() {
                Some('|') => {
                    self.bump();
                    Tok::OrOr
                }
                Some('>') => {
                    self.bump();
                    Tok::PipeArrow
                }
                _ => Tok::Pipe,
            },
            '=' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Tok::EqEq
                } else {
                    Tok::Assign
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Tok::Ne
                } else {
                    Tok::Bang
                }
            }
            '<' => match self.peek() {
                Some('=') => {
                    self.bump();
                    Tok::Le
                }
                Some('<') => {
                    self.bump();
                    Tok::Shl
                }
                _ => Tok::Lt,
            },
            '>' => match self.peek() {
                Some('=') => {
                    self.bump();
                    Tok::Ge
                }
                Some('>') => {
                    self.bump();
                    Tok::Shr
                }
                _ => Tok::Gt,
            },
            other => {
                diags.push(
                    Diagnostic::error(
                        "E0001",
                        Span::new(start, self.pos()),
                        format!("неизвестный символ `{other}`"),
                    )
                    .with_hint("удалите символ или проверьте раскладку"),
                );
                return None;
            }
        };
        Some(t)
    }
}
