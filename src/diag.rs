//! Диагностики Goraw в формате, удобном для LLM: структурированный JSON
//! с «нотками XML» — каждая диагностика несёт машиночитаемые поля и
//! человекочитаемое `<explain>`. Компилятор никогда не паникует на
//! пользовательской ошибке: он копит диагностики и печатает их пачкой.

use std::fmt::Write as _;

/// Позиция в исходнике: смещение в байтах + строка/столбец (1-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub offset: usize,
    pub line: u32,
    pub col: u32,
}

impl Pos {
    pub const ZERO: Pos = Pos { offset: 0, line: 1, col: 1 };
}

/// Полуинтервал [lo, hi) внутри одного файла.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub lo: Pos,
    pub hi: Pos,
}

impl Span {
    pub fn new(lo: Pos, hi: Pos) -> Span {
        Span { lo, hi }
    }
    pub fn dummy() -> Span {
        Span { lo: Pos::ZERO, hi: Pos::ZERO }
    }
    /// Объединяет два спана в наименьший объемлющий.
    pub fn to(self, other: Span) -> Span {
        Span { lo: self.lo, hi: other.hi }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// Одна диагностика. `code` — стабильный идентификатор вида "E0102",
/// по которому LLM/инструменты могут группировать и реагировать.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    /// Короткая подсказка «как чинить» — отдельное поле, чтобы модель
    /// могла её применить, не парся человеческий текст.
    pub hint: Option<String>,
    /// Дополнительные помеченные места («здесь объявлено», «ожидалось тут»).
    pub notes: Vec<(String, Span)>,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
            hint: None,
            notes: Vec::new(),
        }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
            hint: None,
            notes: Vec::new(),
        }
    }


    pub fn with_hint(mut self, hint: impl Into<String>) -> Diagnostic {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_note(mut self, message: impl Into<String>, span: Span) -> Diagnostic {
        self.notes.push((message.into(), span));
        self
    }
}

/// Копилка диагностик + исходный текст для извлечения строк-контекста.
pub struct Diags {
    pub file: String,
    pub src: String,
    pub items: Vec<Diagnostic>,
    /// Для мульти-файловой сборки: (стартовая строка в объединённом src, путь).
    /// Пусто → один файл. Отсортировано по стартовой строке.
    pub line_map: Vec<(u32, String)>,
}

impl Diags {
    pub fn new(file: impl Into<String>, src: impl Into<String>) -> Diags {
        Diags { file: file.into(), src: src.into(), items: Vec::new(), line_map: Vec::new() }
    }

    /// Устанавливает карту файлов для объединённого источника.
    pub fn set_line_map(&mut self, map: Vec<(u32, String)>) {
        self.line_map = map;
    }

    /// По глобальной строке в объединённом src возвращает (путь, локальная строка).
    pub fn locate(&self, line: u32) -> (String, u32) {
        if self.line_map.is_empty() {
            return (self.file.clone(), line);
        }
        let mut best = &self.line_map[0];
        for e in &self.line_map {
            if e.0 <= line {
                best = e;
            } else {
                break;
            }
        }
        (best.1.clone(), line.saturating_sub(best.0) + 1)
    }

    pub fn push(&mut self, d: Diagnostic) {
        const MAX_DIAGNOSTICS_CAP: usize = 50;
        if self.items.len() >= MAX_DIAGNOSTICS_CAP {
            if self.items.len() == MAX_DIAGNOSTICS_CAP {
                eprintln!(
                    "[CIRCUIT BREAKER] Превышен лимит ошибок (>{MAX_DIAGNOSTICS_CAP}), аварийная остановка потока компилятора!"
                );
                self.items.push(Diagnostic::error(
                    "E9999",
                    d.span,
                    "превышен лимит ошибок (circuit breaker), дальнейший разбор остановлен во избежание утечки памяти",
                ));
            }
            eprint!("{}", self.render_human());
            std::process::exit(1);
        }
        self.items.push(d);
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.items.iter().filter(|d| d.severity == Severity::Error).count()
    }

    /// Строка исходника (1-based) без завершающего перевода строки.
    fn line_text(&self, line: u32) -> &str {
        self.src.lines().nth(line.saturating_sub(1) as usize).unwrap_or("")
    }

    /// Человекочитаемый вывод для терминала (когда LLM не при делах).
    pub fn render_human(&self) -> String {
        let mut out = String::new();
        for d in &self.items {
            let _ = writeln!(
                out,
                "{sev}[{code}]: {msg}",
                sev = d.severity.as_str(),
                code = d.code,
                msg = d.message
            );
            let (path, local_line) = self.locate(d.span.lo.line);
            let _ = writeln!(out, "  --> {path}:{local_line}:{col}", col = d.span.lo.col);
            let line = self.line_text(d.span.lo.line);
            let _ = writeln!(out, "   | {line}");
            let caret_pad = d.span.lo.col.saturating_sub(1) as usize;
            let width = if d.span.hi.line == d.span.lo.line {
                (d.span.hi.col.saturating_sub(d.span.lo.col)).max(1) as usize
            } else {
                1
            };
            let _ = writeln!(out, "   | {}{}", " ".repeat(caret_pad), "^".repeat(width));
            for (msg, sp) in &d.notes {
                let _ = writeln!(out, "   = note: {msg} (at {}:{})", sp.lo.line, sp.lo.col);
            }
            if let Some(h) = &d.hint {
                let _ = writeln!(out, "   = hint: {h}");
            }
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "{} error(s), {} warning(s)",
            self.error_count(),
            self.items.len() - self.error_count()
        );
        out
    }

    /// Машиночитаемый вывод: JSON-объект, где каждая диагностика содержит
    /// вложенный XML-фрагмент `<explain>` с контекстом строки и кареткой.
    /// Такой гибрид проще всего скармливать модели: верхний уровень —
    /// строгий JSON для парсинга, `explain` — размеченный текст для чтения.
    pub fn render_llm_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        let _ = writeln!(out, "  \"schema\": \"goraw.diagnostics/v1\",");
        let _ = writeln!(out, "  \"file\": {},", jstr(&self.file));
        let _ = writeln!(out, "  \"ok\": {},", !self.has_errors());
        let _ = writeln!(out, "  \"errors\": {},", self.error_count());
        let _ = writeln!(
            out,
            "  \"warnings\": {},",
            self.items.len() - self.error_count()
        );
        out.push_str("  \"diagnostics\": [\n");
        for (i, d) in self.items.iter().enumerate() {
            let line = self.line_text(d.span.lo.line);
            let caret_pad = d.span.lo.col.saturating_sub(1) as usize;
            let width = if d.span.hi.line == d.span.lo.line {
                (d.span.hi.col.saturating_sub(d.span.lo.col)).max(1) as usize
            } else {
                1
            };
            let (path, local_line) = self.locate(d.span.lo.line);
            let explain = format!(
                "<explain code=\"{code}\" severity=\"{sev}\">\n  <at file=\"{fpath}\" line=\"{ln}\" col=\"{col}\"/>\n  <source>{src}</source>\n  <mark>{pad}{car}</mark>\n  <message>{msg}</message>{hint}\n</explain>",
                code = d.code,
                sev = d.severity.as_str(),
                fpath = xesc(&path),
                ln = local_line,
                col = d.span.lo.col,
                src = xesc(line),
                pad = " ".repeat(caret_pad),
                car = "^".repeat(width),
                msg = xesc(&d.message),
                hint = d
                    .hint
                    .as_ref()
                    .map(|h| format!("\n  <hint>{}</hint>", xesc(h)))
                    .unwrap_or_default(),
            );

            out.push_str("    {\n");
            let _ = writeln!(out, "      \"severity\": {},", jstr(d.severity.as_str()));
            let _ = writeln!(out, "      \"code\": {},", jstr(d.code));
            let _ = writeln!(out, "      \"message\": {},", jstr(&d.message));
            let _ = writeln!(out, "      \"srcfile\": {},", jstr(&path));
            let _ = writeln!(
                out,
                "      \"line\": {}, \"col\": {}, \"endLine\": {}, \"endCol\": {},",
                local_line, d.span.lo.col, d.span.hi.line, d.span.hi.col
            );
            match &d.hint {
                Some(h) => {
                    let _ = writeln!(out, "      \"hint\": {},", jstr(h));
                }
                None => {
                    let _ = writeln!(out, "      \"hint\": null,");
                }
            }
            // notes
            out.push_str("      \"notes\": [");
            for (n, (msg, sp)) in d.notes.iter().enumerate() {
                if n > 0 {
                    out.push_str(", ");
                }
                let _ = write!(
                    out,
                    "{{\"message\": {}, \"line\": {}, \"col\": {}}}",
                    jstr(msg),
                    sp.lo.line,
                    sp.lo.col
                );
            }
            out.push_str("],\n");
            let _ = writeln!(out, "      \"explain\": {}", jstr(&explain));
            out.push_str("    }");
            if i + 1 != self.items.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ]\n}\n");
        out
    }
}

/// JSON-экранирование строки с кавычками.
fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// XML-экранирование для содержимого `<explain>`.
fn xesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
