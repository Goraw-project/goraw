//! Парсер Goraw: рекурсивный спуск + Pratt для выражений.
//! Методы возвращают `Option`, где `None` означает «ошибка уже в Diags,
//! разматываемся до ближайшей точки синхронизации». Это позволяет за один
//! проход собрать несколько ошибок, а не падать на первой.

use crate::ast::*;
use crate::diag::{Diagnostic, Diags, Span};
use crate::lexer::{Tok, Token};

pub struct Parser<'a> {
    toks: Vec<Token>,
    i: usize,
    src: &'a str,
    diags: &'a mut Diags,
    /// Пока true, идентификатор перед `{` НЕ считается началом
    /// литерала структуры (нужно для `if x {`, `for x {`).
    no_struct_lit: bool,
}

type P<T> = Option<T>;

impl<'a> Parser<'a> {
    pub fn new(toks: Vec<Token>, src: &'a str, diags: &'a mut Diags) -> Parser<'a> {
        Parser { toks, i: 0, src, diags, no_struct_lit: false }
    }

    // ---- низкоуровневые помощники ----

    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }
    fn peek2(&self) -> &Tok {
        self.toks.get(self.i + 1).map(|t| &t.tok).unwrap_or(&Tok::Eof)
    }
    fn span(&self) -> Span {
        self.toks[self.i].span
    }
    fn prev_span(&self) -> Span {
        self.toks[self.i.saturating_sub(1)].span
    }
    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.i].clone();
        if !self.at_eof() {
            self.i += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> P<()> {
        if self.peek() == t {
            self.bump();
            Some(())
        } else {
            self.diags.push(
                Diagnostic::error(
                    "E0010",
                    self.span(),
                    format!("ожидалось {what}, а найдено {}", describe(self.peek())),
                )
                .with_hint(format!("добавьте {what}")),
            );
            None
        }
    }

    fn expect_ident(&mut self, what: &str) -> P<(String, Span)> {
        let sp = self.span();
        if let Tok::Ident(name) = self.peek().clone() {
            self.bump();
            Some((name, sp))
        } else {
            self.diags.push(Diagnostic::error(
                "E0011",
                sp,
                format!("ожидалось имя {what}, а найдено {}", describe(self.peek())),
            ));
            None
        }
    }

    /// Пропуск токенов до вероятной границы (для восстановления).
    fn synchronize(&mut self) {
        while !self.at_eof() {
            match self.peek() {
                Tok::Semi => {
                    self.bump();
                    return;
                }
                Tok::RBrace | Tok::Fn | Tok::Struct | Tok::Extern => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    // ---- верхний уровень ----

    pub fn parse_program(&mut self) -> Program {
        let mut structs = Vec::new();
        let mut fns = Vec::new();
        while !self.at_eof() {
            match self.peek() {
                Tok::Struct => match self.parse_struct() {
                    Some(s) => structs.push(s),
                    None => self.synchronize(),
                },
                Tok::Fn | Tok::Extern | Tok::Unsafe => match self.parse_fn() {
                    Some(f) => fns.push(f),
                    None => self.synchronize(),
                },
                _ => {
                    self.diags.push(
                        Diagnostic::error(
                            "E0012",
                            self.span(),
                            format!(
                                "на верхнем уровне ожидалось `fn`, `struct` или `extern`, а найдено {}",
                                describe(self.peek())
                            ),
                        )
                        .with_hint("объявления верхнего уровня начинаются с `fn`, `struct`, `extern`"),
                    );
                    self.synchronize();
                }
            }
        }
        Program { structs, fns }
    }

    fn parse_struct(&mut self) -> P<StructDef> {
        let start = self.span();
        self.expect(&Tok::Struct, "`struct`")?;
        let (name, _) = self.expect_ident("структуры")?;
        self.expect(&Tok::LBrace, "`{`")?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
            let fsp = self.span();
            let (fname, _) = self.expect_ident("поля")?;
            self.expect(&Tok::Colon, "`:`")?;
            let ty = self.parse_type()?;
            fields.push(Param { name: fname, ty, span: fsp });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let end = self.span();
        self.expect(&Tok::RBrace, "`}`")?;
        Some(StructDef { name, fields, span: start.to(end) })
    }

    fn parse_fn(&mut self) -> P<FnDef> {
        let start = self.span();
        let is_extern = self.eat(&Tok::Extern);
        let is_unsafe = self.eat(&Tok::Unsafe);
        self.expect(&Tok::Fn, "`fn`")?;
        let (name, _) = self.expect_ident("функции")?;
        self.expect(&Tok::LParen, "`(`")?;
        let mut params = Vec::new();
        let mut variadic = false;
        while !matches!(self.peek(), Tok::RParen | Tok::Eof) {
            if self.eat(&Tok::Ellipsis) {
                variadic = true;
                break;
            }
            let psp = self.span();
            let (pname, _) = self.expect_ident("параметра")?;
            self.expect(&Tok::Colon, "`:`")?;
            let ty = self.parse_type()?;
            params.push(Param { name: pname, ty, span: psp });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen, "`)`")?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.parse_type()?) } else { None };

        let body = if is_extern {
            self.expect(&Tok::Semi, "`;` после extern-объявления")?;
            None
        } else {
            Some(self.parse_block()?)
        };
        let end = self.prev_span();
        Some(FnDef {
            name,
            params,
            variadic,
            ret,
            body,
            is_unsafe,
            is_extern,
            span: start.to(end),
        })
    }

    fn parse_type(&mut self) -> P<TypeExpr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Star => {
                self.bump();
                if self.eat(&Tok::Mut) {
                    let inner = self.parse_type()?;
                    Some(TypeExpr::PtrMut(Box::new(inner), sp.to(self.prev_span())))
                } else {
                    let inner = self.parse_type()?;
                    Some(TypeExpr::Ptr(Box::new(inner), sp.to(self.prev_span())))
                }
            }
            Tok::Ident(name) => {
                self.bump();
                Some(TypeExpr::Named(name, sp))
            }
            other => {
                self.diags.push(Diagnostic::error(
                    "E0013",
                    sp,
                    format!("ожидался тип, а найдено {}", describe(&other)),
                ));
                None
            }
        }
    }

    // ---- блоки и инструкции ----

    fn parse_block(&mut self) -> P<Block> {
        let start = self.span();
        self.expect(&Tok::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
            match self.parse_stmt() {
                Some(s) => stmts.push(s),
                None => {
                    self.synchronize();
                    if matches!(self.peek(), Tok::RBrace | Tok::Eof) {
                        break;
                    }
                }
            }
        }
        let end = self.span();
        self.expect(&Tok::RBrace, "`}`")?;
        Some(Block { stmts, span: start.to(end) })
    }

    fn parse_stmt(&mut self) -> P<Stmt> {
        match self.peek() {
            Tok::Return => {
                let sp = self.span();
                self.bump();
                let value = if matches!(self.peek(), Tok::Semi) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(&Tok::Semi, "`;`")?;
                Some(Stmt::Return(value, sp.to(self.prev_span())))
            }
            Tok::If => self.parse_if(),
            Tok::While => {
                let sp = self.span();
                self.bump();
                self.no_struct_lit = true;
                let cond = self.parse_expr();
                self.no_struct_lit = false;
                let cond = cond?;
                let body = self.parse_block()?;
                Some(Stmt::While { cond, body, span: sp.to(self.prev_span()) })
            }
            Tok::For => self.parse_for(),
            Tok::Break => {
                let sp = self.span();
                self.bump();
                self.expect(&Tok::Semi, "`;`")?;
                Some(Stmt::Break(sp))
            }
            Tok::Continue => {
                let sp = self.span();
                self.bump();
                self.expect(&Tok::Semi, "`;`")?;
                Some(Stmt::Continue(sp))
            }
            Tok::Unsafe => {
                let sp = self.span();
                self.bump();
                let block = self.parse_block()?;
                Some(Stmt::Unsafe(block, sp.to(self.prev_span())))
            }
            Tok::Asm => {
                let a = self.parse_asm()?;
                self.expect(&Tok::Semi, "`;`")?;
                Some(Stmt::Asm(a))
            }
            _ => {
                let s = self.parse_simple_stmt()?;
                self.expect(&Tok::Semi, "`;`")?;
                Some(s)
            }
        }
    }

    fn parse_if(&mut self) -> P<Stmt> {
        let sp = self.span();
        self.expect(&Tok::If, "`if`")?;
        self.no_struct_lit = true;
        let cond = self.parse_expr();
        self.no_struct_lit = false;
        let cond = cond?;
        let then = self.parse_block()?;
        let els = if self.eat(&Tok::Else) {
            if matches!(self.peek(), Tok::If) {
                // else if — оборачиваем в блок из одной инструкции
                let inner = self.parse_if()?;
                let sp2 = inner.span();
                Some(Block { stmts: vec![inner], span: sp2 })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Some(Stmt::If { cond, then, els, span: sp.to(self.prev_span()) })
    }

    fn parse_for(&mut self) -> P<Stmt> {
        let sp = self.span();
        self.expect(&Tok::For, "`for`")?;
        self.no_struct_lit = true;

        // for { }  — бесконечный цикл
        if matches!(self.peek(), Tok::LBrace) {
            self.no_struct_lit = false;
            let body = self.parse_block()?;
            return Some(Stmt::For { init: None, cond: None, post: None, body, span: sp.to(self.prev_span()) });
        }

        let init = if matches!(self.peek(), Tok::Semi) {
            None
        } else {
            Some(Box::new(self.parse_simple_stmt()?))
        };

        if matches!(self.peek(), Tok::Semi) {
            // трёхчастная форма: init; cond; post
            self.bump(); // ;
            let cond = if matches!(self.peek(), Tok::Semi) { None } else { Some(self.parse_expr()?) };
            self.expect(&Tok::Semi, "`;`")?;
            let post = if matches!(self.peek(), Tok::LBrace) {
                None
            } else {
                Some(Box::new(self.parse_simple_stmt()?))
            };
            self.no_struct_lit = false;
            let body = self.parse_block()?;
            Some(Stmt::For { init, cond, post, body, span: sp.to(self.prev_span()) })
        } else {
            // while-форма: for <cond> { }
            self.no_struct_lit = false;
            let cond = match init {
                Some(b) => match *b {
                    Stmt::Expr(e) => Some(e),
                    other => {
                        self.diags.push(Diagnostic::error(
                            "E0014",
                            other.span(),
                            "в `for cond { }` ожидалось выражение-условие",
                        ).with_hint("для трёхчастного цикла добавьте `;`: `for init; cond; post { }`"));
                        return None;
                    }
                },
                None => None,
            };
            let body = self.parse_block()?;
            Some(Stmt::For { init: None, cond, post: None, body, span: sp.to(self.prev_span()) })
        }
    }

    /// Простая инструкция без завершающего `;`: let / короткое объявление /
    /// присваивание / `x++` / `x--` / выражение.
    fn parse_simple_stmt(&mut self) -> P<Stmt> {
        // let [mut] name [: T] = expr
        if matches!(self.peek(), Tok::Let) {
            let sp = self.span();
            self.bump();
            let mutable = self.eat(&Tok::Mut);
            let (name, _) = self.expect_ident("переменной")?;
            let ty = if self.eat(&Tok::Colon) { Some(self.parse_type()?) } else { None };
            self.expect(&Tok::Assign, "`=`")?;
            let value = self.parse_expr()?;
            return Some(Stmt::Let { name, mutable, ty, value, span: sp.to(self.prev_span()) });
        }

        // Go-style short decl:  name := expr
        if let Tok::Ident(name) = self.peek().clone() {
            if matches!(self.peek2(), Tok::ColonEq) {
                let sp = self.span();
                self.bump(); // ident
                self.bump(); // :=
                let value = self.parse_expr()?;
                return Some(Stmt::Let { name, mutable: true, ty: None, value, span: sp.to(self.prev_span()) });
            }
        }

        // иначе: выражение, затем возможно =, ++ или --
        let e = self.parse_expr()?;
        match self.peek() {
            Tok::Assign => {
                self.bump();
                let value = self.parse_expr()?;
                Some(Stmt::Assign { span: e.span().to(value.span()), target: e, value })
            }
            Tok::PlusPlus => {
                let sp = self.span();
                self.bump();
                let one = Expr::Int(1, sp);
                let value = Expr::Binary { op: BinOp::Add, lhs: Box::new(e.clone()), rhs: Box::new(one), span: e.span().to(sp) };
                Some(Stmt::Assign { span: e.span().to(sp), target: e, value })
            }
            Tok::MinusMinus => {
                let sp = self.span();
                self.bump();
                let one = Expr::Int(1, sp);
                let value = Expr::Binary { op: BinOp::Sub, lhs: Box::new(e.clone()), rhs: Box::new(one), span: e.span().to(sp) };
                Some(Stmt::Assign { span: e.span().to(sp), target: e, value })
            }
            _ => Some(Stmt::Expr(e)),
        }
    }

    // ---- инлайн-ассемблер (тело захватывается как сырой текст) ----

    fn parse_asm(&mut self) -> P<AsmBlock> {
        let sp = self.span();
        self.expect(&Tok::Asm, "`asm`")?;
        self.expect(&Tok::LParen, "`(`")?;
        // диалект: строковый литерал "masm" | "nasm"
        let dialect = match self.peek().clone() {
            Tok::Str(s) => {
                self.bump();
                match s.as_str() {
                    "masm" | "intel" => AsmDialect::Masm,
                    "nasm" => AsmDialect::Nasm,
                    other => {
                        self.diags.push(Diagnostic::error(
                            "E0015",
                            sp,
                            format!("неизвестный диалект ассемблера `{other}`"),
                        ).with_hint("поддерживаются `masm` и `nasm`"));
                        AsmDialect::Masm
                    }
                }
            }
            _ => {
                self.diags.push(Diagnostic::error("E0015", self.span(), "ожидался диалект: \"masm\" или \"nasm\""));
                AsmDialect::Masm
            }
        };
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        while self.eat(&Tok::Comma) {
            let (key, _) = self.expect_ident("`inputs`/`outputs`")?;
            self.expect(&Tok::Colon, "`:`")?;
            self.expect(&Tok::LBracket, "`[`")?;
            let mut list = Vec::new();
            while !matches!(self.peek(), Tok::RBracket | Tok::Eof) {
                let (n, _) = self.expect_ident("переменной")?;
                list.push(n);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RBracket, "`]`")?;
            match key.as_str() {
                "inputs" => inputs = list,
                "outputs" => outputs = list,
                other => {
                    self.diags.push(Diagnostic::error(
                        "E0016",
                        sp,
                        format!("неизвестный параметр asm `{other}`"),
                    ).with_hint("допустимы `inputs` и `outputs`"));
                }
            }
        }
        self.expect(&Tok::RParen, "`)`")?;

        // Тело: захватываем сырой текст между `{` и парной `}` по смещениям.
        let lbrace = self.span();
        self.expect(&Tok::LBrace, "`{`")?;
        let body_start = lbrace.hi.offset; // байт сразу после `{`
        let mut depth = 1;
        while depth > 0 && !self.at_eof() {
            match self.peek() {
                Tok::LBrace => depth += 1,
                Tok::RBrace => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            self.bump();
        }
        let body_end = self.span().lo.offset; // байт перед закрывающей `}`
        let body = self.src.get(body_start..body_end).unwrap_or("").trim().to_string();
        self.expect(&Tok::RBrace, "`}`")?;
        Some(AsmBlock { dialect, inputs, outputs, body, span: sp.to(self.prev_span()) })
    }

    // ---- выражения (Pratt) ----

    pub fn parse_expr(&mut self) -> P<Expr> {
        self.parse_pipeline()
    }

    /// Конвейер `x |> f(a)` == `f(x, a)`; `x |> f` == `f(x)`. Лево-ассоциативен
    /// и связывает слабее любой арифметики, так что `a + b |> f` == `f(a + b)`.
    fn parse_pipeline(&mut self) -> P<Expr> {
        let mut lhs = self.parse_binary(0)?;
        while matches!(self.peek(), Tok::PipeArrow) {
            let opsp = self.span();
            self.bump();
            let rhs = self.parse_unary()?; // функция или вызов (с постфиксами)
            lhs = match rhs {
                Expr::Call { callee, mut args, span } => {
                    let mut newargs = Vec::with_capacity(args.len() + 1);
                    newargs.push(lhs);
                    newargs.append(&mut args);
                    Expr::Call { callee, args: newargs, span }
                }
                Expr::Ident(..) => {
                    let span = lhs.span().to(rhs.span());
                    Expr::Call { callee: Box::new(rhs), args: vec![lhs], span }
                }
                other => {
                    self.diags.push(
                        Diagnostic::error(
                            "E0018",
                            opsp.to(other.span()),
                            "справа от `|>` ожидается функция или вызов",
                        )
                        .with_hint("напр. `x |> sqrt` или `x |> pow(2.0)`"),
                    );
                    return None;
                }
            };
        }
        Some(lhs)
    }

    fn parse_binary(&mut self, min_prec: u8) -> P<Expr> {
        let mut lhs = self.parse_cast()?;
        loop {
            let (op, prec) = match bin_op(self.peek()) {
                Some(x) => x,
                None => break,
            };
            if prec < min_prec {
                break;
            }
            self.bump();
            // все наши бинарные операторы левоассоциативны
            let rhs = self.parse_binary(prec + 1)?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Some(lhs)
    }

    fn parse_cast(&mut self) -> P<Expr> {
        let mut e = self.parse_unary()?;
        while matches!(self.peek(), Tok::As) {
            self.bump();
            let ty = self.parse_type()?;
            let span = e.span().to(ty.span());
            e = Expr::Cast { expr: Box::new(e), ty, span };
        }
        Some(e)
    }

    fn parse_unary(&mut self) -> P<Expr> {
        let sp = self.span();
        match self.peek() {
            Tok::Minus => {
                self.bump();
                let e = self.parse_unary()?;
                Some(Expr::Unary { op: UnOp::Neg, span: sp.to(e.span()), expr: Box::new(e) })
            }
            Tok::Bang => {
                self.bump();
                let e = self.parse_unary()?;
                Some(Expr::Unary { op: UnOp::Not, span: sp.to(e.span()), expr: Box::new(e) })
            }
            Tok::Star => {
                self.bump();
                let e = self.parse_unary()?;
                Some(Expr::Unary { op: UnOp::Deref, span: sp.to(e.span()), expr: Box::new(e) })
            }
            Tok::Amp => {
                self.bump();
                if self.eat(&Tok::Mut) {
                    let e = self.parse_unary()?;
                    Some(Expr::Unary { op: UnOp::RefMut, span: sp.to(e.span()), expr: Box::new(e) })
                } else {
                    let e = self.parse_unary()?;
                    Some(Expr::Unary { op: UnOp::Ref, span: sp.to(e.span()), expr: Box::new(e) })
                }
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> P<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    // аргументы вызова разбираем в обычном режиме (struct-литералы разрешены)
                    let saved = self.no_struct_lit;
                    self.no_struct_lit = false;
                    while !matches!(self.peek(), Tok::RParen | Tok::Eof) {
                        args.push(self.parse_expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.no_struct_lit = saved;
                    let end = self.span();
                    self.expect(&Tok::RParen, "`)`")?;
                    e = Expr::Call { span: e.span().to(end), callee: Box::new(e), args };
                }
                Tok::LBracket => {
                    self.bump();
                    let saved = self.no_struct_lit;
                    self.no_struct_lit = false;
                    let index = self.parse_expr()?;
                    self.no_struct_lit = saved;
                    let end = self.span();
                    self.expect(&Tok::RBracket, "`]`")?;
                    e = Expr::Index { span: e.span().to(end), base: Box::new(e), index: Box::new(index) };
                }
                Tok::Dot => {
                    self.bump();
                    let (field, fsp) = self.expect_ident("поля")?;
                    e = Expr::Field { span: e.span().to(fsp), base: Box::new(e), field };
                }
                _ => break,
            }
        }
        Some(e)
    }

    fn parse_primary(&mut self) -> P<Expr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                Some(Expr::Int(n, sp))
            }
            Tok::Float(f) => {
                self.bump();
                Some(Expr::Float(f, sp))
            }
            Tok::Str(s) => {
                self.bump();
                Some(Expr::Str(s, sp))
            }
            Tok::True => {
                self.bump();
                Some(Expr::Bool(true, sp))
            }
            Tok::False => {
                self.bump();
                Some(Expr::Bool(false, sp))
            }
            Tok::LParen => {
                self.bump();
                let saved = self.no_struct_lit;
                self.no_struct_lit = false;
                let e = self.parse_expr()?;
                self.no_struct_lit = saved;
                self.expect(&Tok::RParen, "`)`")?;
                Some(e)
            }
            Tok::Ident(name) => {
                self.bump();
                // литерал структуры Name { ... }
                if !self.no_struct_lit && matches!(self.peek(), Tok::LBrace) {
                    self.bump();
                    let mut fields = Vec::new();
                    while !matches!(self.peek(), Tok::RBrace | Tok::Eof) {
                        let (fname, fsp) = self.expect_ident("поля")?;
                        self.expect(&Tok::Colon, "`:`")?;
                        let val = self.parse_expr()?;
                        fields.push((fname, val, fsp));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    let end = self.span();
                    self.expect(&Tok::RBrace, "`}`")?;
                    Some(Expr::StructLit { name, fields, span: sp.to(end) })
                } else {
                    Some(Expr::Ident(name, sp))
                }
            }
            other => {
                self.diags.push(
                    Diagnostic::error(
                        "E0017",
                        sp,
                        format!("ожидалось выражение, а найдено {}", describe(&other)),
                    )
                    .with_hint("здесь должно быть значение: число, имя, вызов и т.п."),
                );
                None
            }
        }
    }
}

/// Приоритет и вид бинарного оператора (больше число — выше приоритет).
fn bin_op(t: &Tok) -> Option<(BinOp, u8)> {
    Some(match t {
        Tok::OrOr => (BinOp::Or, 1),
        Tok::AndAnd => (BinOp::And, 2),
        Tok::EqEq => (BinOp::Eq, 3),
        Tok::Ne => (BinOp::Ne, 3),
        Tok::Lt => (BinOp::Lt, 3),
        Tok::Le => (BinOp::Le, 3),
        Tok::Gt => (BinOp::Gt, 3),
        Tok::Ge => (BinOp::Ge, 3),
        Tok::Pipe => (BinOp::BitOr, 4),
        Tok::Caret => (BinOp::BitXor, 5),
        Tok::Amp => (BinOp::BitAnd, 6),
        Tok::Shl => (BinOp::Shl, 7),
        Tok::Shr => (BinOp::Shr, 7),
        Tok::Plus => (BinOp::Add, 8),
        Tok::Minus => (BinOp::Sub, 8),
        Tok::Star => (BinOp::Mul, 9),
        Tok::Slash => (BinOp::Div, 9),
        Tok::Percent => (BinOp::Rem, 9),
        _ => return None,
    })
}

/// Человекочитаемое имя токена для сообщений об ошибках.
fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("идентификатор `{s}`"),
        Tok::Int(n) => format!("число `{n}`"),
        Tok::Float(f) => format!("число `{f}`"),
        Tok::Str(_) => "строку".into(),
        Tok::Eof => "конец файла".into(),
        Tok::LBrace => "`{`".into(),
        Tok::RBrace => "`}`".into(),
        Tok::LParen => "`(`".into(),
        Tok::RParen => "`)`".into(),
        Tok::Semi => "`;`".into(),
        Tok::Colon => "`:`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Fn => "`fn`".into(),
        other => format!("{other:?}"),
    }
}
