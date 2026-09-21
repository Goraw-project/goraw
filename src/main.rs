//! Драйвер компилятора Goraw.
//!
//! Пайплайн: исходник -> лексер -> парсер -> сбор типов -> кодоген (LLVM IR)
//! -> clang -> .exe. Диагностики умеет печатать по-человечески или в
//! LLM-дружественном JSON (`--json`).

use gorawc::{ast, codegen, diag, lexer, parser, types};

use std::path::{Path, PathBuf};
use std::process::{exit, Command};

/// Рантайм JIT-специализации встроен в компилятор и разворачивается рядом с
/// .ll только когда программа реально использует jit-блоки.
const JIT_RUNTIME_C: &str = include_str!("../runtime/goraw_jit.c");

struct Options {
    input: PathBuf,
    output: Option<PathBuf>,
    emit_llvm: bool,   // остановиться на .ll
    json: bool,        // диагностика в JSON
    run: bool,         // запустить после сборки
    opt: Option<String>, // уровень оптимизации, напр. "2"
    clang: String,
    keep_ll: bool,
    test: bool, // собрать и прогнать shadow-тесты
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            exit(2);
        }
    };
    exit(run(opts));
}

fn print_help() {
    println!(
        "gorawc — компилятор языка Goraw (LLVM backend)\n\
\n\
ИСПОЛЬЗОВАНИЕ:\n\
    gorawc <файл.gw> [опции]\n\
\n\
ОПЦИИ:\n\
    -o <путь>        имя выходного файла (.exe или .ll)\n\
    --emit-llvm      остановиться на LLVM IR (.ll), не звать clang\n\
    --json           печатать диагностику в LLM-формате (JSON + XML-нотки)\n\
    --run            запустить программу после успешной сборки\n\
    --test           собрать и прогнать shadow-тесты (test-блоки)\n\
    -O<n>            уровень оптимизации clang (напр. -O2)\n\
    --keep-ll        не удалять промежуточный .ll при сборке .exe\n\
    --clang <путь>   путь к clang (по умолчанию `clang` из PATH)\n\
    -h, --help       показать эту справку\n"
    );
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut input: Option<PathBuf> = None;
    let mut output = None;
    let mut emit_llvm = false;
    let mut json = false;
    let mut run = false;
    let mut opt = None;
    let mut clang = "clang".to_string();
    let mut keep_ll = false;
    let mut test = false;

    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                print_help();
                exit(0);
            }
            "-o" => {
                i += 1;
                output = Some(PathBuf::from(args.get(i).ok_or("-o требует аргумент")?));
            }
            "--emit-llvm" => emit_llvm = true,
            "--json" => json = true,
            "--run" => run = true,
            "--test" => test = true,
            "--keep-ll" => keep_ll = true,
            "--clang" => {
                i += 1;
                clang = args.get(i).ok_or("--clang требует аргумент")?.clone();
            }
            s if s.starts_with("-O") => opt = Some(s[2..].to_string()),
            s if s.starts_with('-') => return Err(format!("неизвестная опция `{s}` (см. --help)")),
            s => {
                if input.is_some() {
                    return Err(format!("лишний аргумент `{s}`"));
                }
                input = Some(PathBuf::from(s));
            }
        }
        i += 1;
    }

    let input = input.ok_or("не указан входной файл (см. --help)")?;
    Ok(Options { input, output, emit_llvm, json, run, opt, clang, keep_ll, test })
}

fn run(opts: Options) -> i32 {
    // Собираем главный файл и все, что он тянет через `import "..."`.
    let (src, line_map) = match gather_sources(&opts.input) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let file = opts.input.display().to_string();
    let mut diags = diag::Diags::new(file.clone(), src.clone());
    diags.set_line_map(line_map);

    // Лексер.
    let mut lx = lexer::Lexer::new(&src);
    let toks = lx.tokenize(&mut diags);

    // Парсер.
    let mut prog = {
        let mut p = parser::Parser::new(toks, &src, &mut diags);
        p.parse_program()
    };

    // Режим тестов: превращаем test-блоки в функции и генерируем harness-main.
    if opts.test {
        transform_tests(&mut prog);
    }

    // Сбор типов (первый проход).
    let mut collected = Vec::new();
    let ctx = types::collect(&prog.structs, &prog.enums, &prog.fns, &mut collected);
    for d in collected {
        diags.push(d);
    }

    // Кодоген + семантика (второй проход).
    let ir = {
        let cg = codegen::Codegen::new(&ctx, &mut diags);
        cg.emit_module(&prog)
    };

    // Есть ошибки — печатаем диагностику и выходим.
    if diags.has_errors() {
        emit_diags(&diags, opts.json);
        return 1;
    }
    // Предупреждения печатаем, но продолжаем.
    if !diags.items.is_empty() {
        emit_diags(&diags, opts.json);
    }

    // Пути вывода.
    let (ll_path, exe_path) = output_paths(&opts);

    if let Err(e) = std::fs::write(&ll_path, &ir) {
        eprintln!("не удалось записать `{}`: {e}", ll_path.display());
        return 2;
    }

    if opts.emit_llvm {
        eprintln!("LLVM IR записан в `{}`", ll_path.display());
        return 0;
    }

    // Если программа использует jit-блоки — рядом кладём C-рантайм и линкуем его.
    let uses_jit = ir.contains("@goraw_jit_compile(");
    let mut jit_rt_path: Option<PathBuf> = None;
    if uses_jit {
        let rt = ll_path.with_extension("jitrt.c");
        if let Err(e) = std::fs::write(&rt, JIT_RUNTIME_C) {
            eprintln!("не удалось записать JIT-рантайм `{}`: {e}", rt.display());
            return 2;
        }
        jit_rt_path = Some(rt);
    }

    // Линковка через clang.
    let mut cmd = Command::new(&opts.clang);
    cmd.arg("--target=x86_64-w64-windows-gnu");
    if let Some(o) = &opts.opt {
        cmd.arg(format!("-O{o}"));
    }
    cmd.arg(&ll_path);
    if let Some(rt) = &jit_rt_path {
        cmd.arg(rt);
    }
    cmd.arg("-o").arg(&exe_path);
    // Подавляем предупреждение о переопределении triple (у нас он корректный).
    cmd.arg("-Wno-override-module");

    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "не удалось запустить clang (`{}`): {e}\nУстановите LLVM или укажите путь через --clang",
                opts.clang
            );
            return 2;
        }
    };
    if !status.success() {
        eprintln!("clang завершился с ошибкой при линковке `{}`", ll_path.display());
        return 1;
    }

    if !opts.keep_ll {
        let _ = std::fs::remove_file(&ll_path);
        if let Some(rt) = &jit_rt_path {
            let _ = std::fs::remove_file(rt);
        }
    }

    eprintln!("собрано: `{}`", exe_path.display());

    if opts.run || opts.test {
        let status = Command::new(&exe_path).status();
        match status {
            Ok(s) => return s.code().unwrap_or(0),
            Err(e) => {
                eprintln!("не удалось запустить `{}`: {e}", exe_path.display());
                return 2;
            }
        }
    }

    0
}

/// Преобразует программу под `--test`: каждый `test`-блок → функция
/// `__test_N() -> i64` (0 = ок, иначе номер строки упавшего assert), плюс
/// сгенерированный `main`, который прогоняет тесты и печатает отчёт. Тесты не
/// попадают в обычную сборку (это «shadow»-тесты) — трансформация только здесь.
fn transform_tests(prog: &mut ast::Program) {
    use ast::*;
    let dummy = diag::Span::dummy();

    // Убираем пользовательский main — его заменит harness.
    let mut fns: Vec<FnDef> = std::mem::take(&mut prog.fns).into_iter().filter(|f| f.name != "main").collect();
    let user_has_printf = fns.iter().any(|f| f.name == "printf");

    let tests = std::mem::take(&mut prog.tests);
    for (i, t) in tests.iter().enumerate() {
        fns.push(FnDef {
            name: format!("__test_{i}"),
            params: Vec::new(),
            variadic: false,
            ret: Some(TypeExpr::Named("i64".into(), dummy)),
            body: Some(t.body.clone()),
            is_unsafe: false,
            is_extern: false,
            is_test: true,
            span: t.span,
        });
    }

    // Harness-main генерируем как исходник Goraw и парсим — без ручной сборки AST.
    let mut hs = String::new();
    if !user_has_printf {
        hs.push_str("extern fn printf(fmt: *u8, ...) -> i32;\n");
    }
    hs.push_str("fn main() -> i32 {\n");
    hs.push_str("    let mut __p: i64 = 0;\n    let mut __f: i64 = 0;\n");
    for (i, t) in tests.iter().enumerate() {
        let name = t.name.replace('\\', "\\\\").replace('"', "\\\"");
        hs.push_str(&format!("    let r{i} = __test_{i}();\n"));
        hs.push_str(&format!(
            "    if r{i} != 0 {{ printf(\"[FAIL] %s (строка %lld)\\n\", \"{name}\", r{i}); __f += 1; }} else {{ printf(\"[ ok ] %s\\n\", \"{name}\"); __p += 1; }}\n"
        ));
    }
    hs.push_str("    printf(\"\\n%lld passed, %lld failed\\n\", __p, __f);\n");
    hs.push_str("    return __f as i32;\n}\n");

    let mut hdiags = diag::Diags::new("<test-harness>", hs.clone());
    let htoks = lexer::Lexer::new(&hs).tokenize(&mut hdiags);
    let hprog = parser::Parser::new(htoks, &hs, &mut hdiags).parse_program();
    fns.extend(hprog.fns);

    prog.fns = fns;
}

/// Рекурсивно собирает главный файл и все импортируемые (`import "path";`) в
/// один объединённый источник + карту строк (для диагностик по файлам).
/// Порядок: зависимости раньше импортёра; дубли включаются один раз.
fn gather_sources(main: &Path) -> Result<(String, Vec<(u32, String)>), String> {
    use std::collections::HashSet;
    let mut order: Vec<(String, String)> = Vec::new(); // (отображаемый путь, src)
    let mut seen: HashSet<PathBuf> = HashSet::new();

    fn visit(
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        order: &mut Vec<(String, String)>,
    ) -> Result<(), String> {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(canon.clone()) {
            return Ok(());
        }
        let src = std::fs::read_to_string(path)
            .map_err(|e| format!("не удалось прочитать `{}`: {e}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        for imp in scan_imports(&src) {
            let resolved = dir.join(&imp);
            visit(&resolved, seen, order)?;
        }
        order.push((path.display().to_string(), src));
        Ok(())
    }

    visit(main, &mut seen, &mut order)?;

    let mut combined = String::new();
    let mut map = Vec::new();
    for (path, src) in &order {
        let start_line = combined.matches('\n').count() as u32 + 1;
        map.push((start_line, path.clone()));
        combined.push_str(src);
        if !src.ends_with('\n') {
            combined.push('\n');
        }
    }
    Ok((combined, map))
}

/// Лёгкое сканирование строк на `import "path";` (без полного парсинга).
fn scan_imports(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let line = match raw.find("//") {
            Some(p) => &raw[..p],
            None => raw,
        };
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("import") {
            let rest = rest.trim_start();
            if let Some(start) = rest.find('"') {
                if let Some(end) = rest[start + 1..].find('"') {
                    out.push(rest[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }
    out
}

fn output_paths(opts: &Options) -> (PathBuf, PathBuf) {
    let stem = opts.input.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "out".into());
    let dir = opts.input.parent().unwrap_or(Path::new("."));
    match &opts.output {
        Some(o) => {
            if opts.emit_llvm {
                (o.clone(), o.clone())
            } else {
                let ll = o.with_extension("ll");
                (ll, o.clone())
            }
        }
        None => {
            if opts.emit_llvm {
                (dir.join(format!("{stem}.ll")), dir.join(format!("{stem}.ll")))
            } else {
                (dir.join(format!("{stem}.ll")), dir.join(format!("{stem}.exe")))
            }
        }
    }
}

fn emit_diags(diags: &diag::Diags, json: bool) {
    if json {
        // JSON идёт в stdout (чтобы удобно ловить пайпом), человекочитаемое — в stderr.
        print!("{}", diags.render_llm_json());
    } else {
        eprint!("{}", diags.render_human());
    }
}
