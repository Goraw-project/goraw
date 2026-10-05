//! `gorawas` — драйвер ассемблера Goraw-asm.
//! Собирает `.asm` в COFF-объектник (`.obj`), диагностика — человекочитаемо
//! или в LLM-JSON (`--json`), как у компилятора.

use std::path::PathBuf;
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut json = false;
    let mut link_pe = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "gorawas — ассемблер и нативный компоновщик Goraw-asm (Intel-диалект, backend iced-x86)\n\n\
                     ИСПОЛЬЗОВАНИЕ:\n    gorawas <файл.asm> [-o out.obj] [--exe] [--json]\n\n\
                     ОПЦИИ:\n    -o <путь>        имя выходного файла (.obj или .exe)\n    --exe, --link    скомпоновать готовый автономный Windows PE (.exe)\n    --json           диагностика в JSON формате\n"
                );
                return;
            }
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => output = Some(PathBuf::from(o)),
                    None => {
                        eprintln!("-o требует аргумент");
                        exit(2);
                    }
                }
            }
            "--exe" | "--link" | "--native-linker" | "--native-pe" => link_pe = true,
            "--json" => json = true,
            s if s.starts_with('-') => {
                eprintln!("неизвестная опция `{s}`");
                exit(2);
            }
            s => input = Some(PathBuf::from(s)),
        }
        i += 1;
    }

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("не указан входной .asm файл (см. --help)");
            exit(2);
        }
    };
    let src = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("не удалось прочитать `{}`: {e}", input.display());
            exit(2);
        }
    };

    let file = input.display().to_string();
    let (obj, diags) = gorawc::asm::assemble(&file, &src);

    if !diags.items.is_empty() {
        if json {
            print!("{}", diags.render_llm_json());
        } else {
            eprint!("{}", diags.render_human());
        }
    }

    let obj = match obj {
        Some(o) => o,
        None => exit(1),
    };

    let out = output.unwrap_or_else(|| {
        if link_pe {
            input.with_extension("exe")
        } else {
            input.with_extension("obj")
        }
    });

    if link_pe || out.extension().and_then(|e| e.to_str()) == Some("exe") {
        match gorawc::linker::link_coff_to_pe(&[&obj], Some("main")) {
            Ok(pe_bytes) => {
                if let Err(e) = std::fs::write(&out, &pe_bytes) {
                    eprintln!("не удалось записать `{}`: {e}", out.display());
                    exit(2);
                }
                eprintln!("скомпонован автономный Windows PE (.exe): `{}`", out.display());
                return;
            }
            Err(e) => {
                eprintln!("ошибка компоновщика PE: {e}");
                exit(1);
            }
        }
    }

    if let Err(e) = std::fs::write(&out, &obj) {
        eprintln!("не удалось записать `{}`: {e}", out.display());
        exit(2);
    }
    eprintln!("собрано: `{}`", out.display());
}
