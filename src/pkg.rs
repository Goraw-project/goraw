//! Пакетный менеджер и модульная система Goraw (`goraw get`, `goraw init`, `goraw.toml`).
//!
//! Обеспечивает:
//! - Декларативный манифест `goraw.toml`
//! - Загрузку и версионирование пакетов через `git clone --depth 1` (`goraw get <url>`)
//! - Локальный вендоринг (`./vendor/`) и глобальный кэш пакетов (`~/.goraw/pkg/`)
//! - Автоматический многоуровневый резолвинг `import "package/..."` в `gather_sources`

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub package: Option<PackageConfig>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageConfig {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    pub entry: Option<String>,
    pub authors: Option<Vec<String>>,
    pub description: Option<String>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    Simple(String),
    Detailed {
        path: Option<String>,
        git: Option<String>,
        tag: Option<String>,
        branch: Option<String>,
        rev: Option<String>,
    },
}

/// Путь к глобальному кэшу пакетов `~/.goraw/pkg/`.
pub fn global_pkg_dir() -> PathBuf {
    let base = if let Ok(home) = std::env::var("USERPROFILE") {
        PathBuf::from(home)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else {
        PathBuf::from(".")
    };
    base.join(".goraw").join("pkg")
}

/// Загружает `goraw.toml` из указанной директории, если он существует.
pub fn load_manifest(dir: &Path) -> Option<Manifest> {
    let manifest_path = dir.join("goraw.toml");
    if !manifest_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&manifest_path).ok()?;
    toml::from_str(&content).ok()
}

/// Сохраняет манифест в `goraw.toml` в указанной директории.
pub fn save_manifest(dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let manifest_path = dir.join("goraw.toml");
    let content = toml::to_string_pretty(manifest)
        .map_err(|e| format!("ошибка сериализации goraw.toml: {e}"))?;
    std::fs::write(&manifest_path, content)
        .map_err(|e| format!("не удалось записать `{}`: {e}", manifest_path.display()))
}

/// Инициализирует новый проект Goraw (`goraw init [name]`).
pub fn init_project(name: Option<&str>, target_dir: &Path) -> Result<(), String> {
    let manifest_path = target_dir.join("goraw.toml");
    if manifest_path.exists() {
        return Err(format!("проект уже инициализирован (`{}` существует)", manifest_path.display()));
    }

    let pkg_name = if let Some(n) = name {
        n.to_string()
    } else if let Some(os_name) = target_dir.file_name().and_then(|s| s.to_str()) {
        os_name.to_string()
    } else {
        "goraw_app".to_string()
    };

    let manifest = Manifest {
        package: Some(PackageConfig {
            name: pkg_name.clone(),
            version: "0.1.0".to_string(),
            entry: Some("src/main.gw".to_string()),
            authors: Some(vec!["t0mil0v-rev <lakeg4merx@gmail.com>".to_string()]),
            description: Some(format!("Goraw project {pkg_name}")),
        }),
        dependencies: BTreeMap::new(),
    };

    save_manifest(target_dir, &manifest)?;

    let src_dir = target_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .map_err(|e| format!("не удалось создать директорию `{}`: {e}", src_dir.display()))?;

    let main_gw = src_dir.join("main.gw");
    if !main_gw.exists() {
        let template = format!(
            "extern fn puts(s: *u8) -> i32;\n\nfn main() -> i32 {{\n    puts(\"Hello from {pkg_name}!\");\n    return 0;\n}}\n"
        );
        std::fs::write(&main_gw, template)
            .map_err(|e| format!("не удалось создать `{}`: {e}", main_gw.display()))?;
    }

    let gitignore = target_dir.join(".gitignore");
    if !gitignore.exists() {
        let ignore_content = "/target/\n/vendor/\n*.exe\n*.ll\n*.obj\n";
        let _ = std::fs::write(gitignore, ignore_content);
    }

    eprintln!("инициализирован проект Goraw `{pkg_name}` в `{}`", target_dir.display());
    Ok(())
}

/// Парсит спецификацию зависимости: `github.com/user/repo[@v1.0.0]` -> (url, relative_path, tag)
pub fn parse_dep_spec(spec: &str) -> (String, PathBuf, Option<String>) {
    let (base, tag) = if let Some(idx) = spec.find('@') {
        (&spec[..idx], Some(spec[idx + 1..].to_string()))
    } else {
        (spec, None)
    };

    let (url, rel_path) = if base.starts_with("http://") || base.starts_with("https://") {
        let stripped = base
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches(".git");
        (base.to_string(), PathBuf::from(stripped))
    } else if base.starts_with("git@") {
        let stripped = base
            .trim_start_matches("git@")
            .replace(':', "/")
            .trim_end_matches(".git")
            .to_string();
        (base.to_string(), PathBuf::from(stripped))
    } else {
        // например github.com/user/repo
        let url = format!("https://{base}.git");
        (url, PathBuf::from(base))
    };

    (url, rel_path, tag)
}

/// Клонирует или обновляет git-репозиторий в указанный путь.
pub fn fetch_git_repo(url: &str, tag: Option<&str>, dest: &Path) -> Result<(), String> {
    if dest.exists() && dest.join(".git").exists() {
        eprintln!("зависимость уже загружена в `{}`, обновление...", dest.display());
        let status = Command::new("git")
            .arg("-C")
            .arg(dest)
            .args(["pull", "--ff-only"])
            .status()
            .map_err(|e| format!("не удалось выполнить git pull: {e}"))?;
        if !status.success() {
            eprintln!("предупреждение: git pull завершился с кодом {status}");
        }
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("не удалось создать директорию `{}`: {e}", parent.display()))?;
    }

    eprintln!("скачивание `{url}` в `{}`...", dest.display());
    let mut cmd = Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(t) = tag {
        cmd.arg("-b").arg(t);
    }
    cmd.arg(url).arg(dest);

    let status = cmd
        .status()
        .map_err(|e| format!("не удалось запустить git (проверьте, что git установлен в PATH): {e}"))?;
    if !status.success() {
        return Err(format!("git clone `{url}` завершился с ошибкой (код {status})"));
    }
    Ok(())
}

/// Скачивает зависимость или все зависимости проекта (`goraw get [url]`).
pub fn get(dep_spec: Option<&str>, is_global: bool) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("не удалось получить cwd: {e}"))?;
    let target_base = if is_global {
        global_pkg_dir()
    } else {
        cwd.join("vendor")
    };

    if let Some(spec) = dep_spec {
        let (url, rel_path, tag) = parse_dep_spec(spec);
        let dest = target_base.join(&rel_path);
        fetch_git_repo(&url, tag.as_deref(), &dest)?;

        // Если есть локальный goraw.toml, добавляем зависимость
        if let Some(mut manifest) = load_manifest(&cwd) {
            let dep_val = if let Some(t) = tag {
                Dependency::Detailed {
                    path: None,
                    git: Some(url),
                    tag: Some(t),
                    branch: None,
                    rev: None,
                }
            } else {
                Dependency::Simple(url)
            };
            let key = rel_path.to_string_lossy().replace('\\', "/");
            manifest.dependencies.insert(key, dep_val);
            let _ = save_manifest(&cwd, &manifest);
        }
        eprintln!("пакет `{}` успешно установлен в `{}`", spec, dest.display());
        return Ok(());
    }

    // Если аргумент не указан, скачиваем всё из goraw.toml
    let manifest = load_manifest(&cwd)
        .ok_or_else(|| "не найден `goraw.toml` (используйте `goraw init` или `goraw get <url>`)".to_string())?;

    if manifest.dependencies.is_empty() {
        eprintln!("в `goraw.toml` нет зависимостей.");
        return Ok(());
    }

    for (name, dep) in &manifest.dependencies {
        match dep {
            Dependency::Simple(spec) => {
                let (url, rel_path, tag) = parse_dep_spec(spec);
                let dest = target_base.join(if !rel_path.as_os_str().is_empty() {
                    &rel_path
                } else {
                    Path::new(name)
                });
                fetch_git_repo(&url, tag.as_deref(), &dest)?;
            }
            Dependency::Detailed { path, git, tag, branch, .. } => {
                if let Some(p) = path {
                    eprintln!("локальная зависимость `{name}`: `{p}`");
                    continue;
                }
                if let Some(url) = git {
                    let dest = target_base.join(name);
                    let target_tag = tag.as_deref().or(branch.as_deref());
                    fetch_git_repo(url, target_tag, &dest)?;
                }
            }
        }
    }

    eprintln!("все зависимости успешно обновлены.");
    Ok(())
}

/// Находит входную точку для указанной директории пакета.
pub fn find_dir_entry(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    // 1. Проверяем goraw.toml в пакете
    if let Some(manifest) = load_manifest(dir) {
        if let Some(pkg) = manifest.package {
            if let Some(entry) = pkg.entry {
                let candidate = dir.join(entry);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    // 2. Стандартные соглашения
    let candidates = [
        "lib.gw",
        "mod.gw",
        "main.gw",
        "src/lib.gw",
        "src/mod.gw",
        "src/main.gw",
    ];
    for c in candidates {
        let p = dir.join(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Интеллектуальный поиск и резолвинг пути импорта `import "path";` по модулям и вендорам.
pub fn resolve_import(dir: &Path, imp: &str) -> Option<PathBuf> {
    let imp_path = Path::new(imp);

    // Список базовых каталогов для поиска
    let mut search_dirs = Vec::new();
    search_dirs.push(dir.to_path_buf());

    if let Ok(cwd) = std::env::current_dir() {
        search_dirs.push(cwd.clone());
        search_dirs.push(cwd.join("vendor"));

        // Проверяем алиасы зависимостей из goraw.toml
        if let Some(manifest) = load_manifest(&cwd) {
            for (dep_name, dep) in &manifest.dependencies {
                if imp == dep_name || imp.starts_with(&format!("{dep_name}/")) {
                    let sub_path = imp.strip_prefix(dep_name).unwrap_or("").trim_start_matches('/');
                    match dep {
                        Dependency::Simple(_) => {
                            let dep_dir = cwd.join("vendor").join(dep_name);
                            if sub_path.is_empty() {
                                if let Some(e) = find_dir_entry(&dep_dir) {
                                    return Some(e);
                                }
                            } else {
                                let target = dep_dir.join(sub_path);
                                if target.exists() {
                                    return Some(target);
                                }
                                let with_gw = dep_dir.join(format!("{sub_path}.gw"));
                                if with_gw.exists() {
                                    return Some(with_gw);
                                }
                            }
                        }
                        Dependency::Detailed { path, .. } => {
                            if let Some(p) = path {
                                let local_dir = cwd.join(p);
                                if sub_path.is_empty() {
                                    if let Some(e) = find_dir_entry(&local_dir) {
                                        return Some(e);
                                    }
                                } else {
                                    let target = local_dir.join(sub_path);
                                    if target.exists() {
                                        return Some(target);
                                    }
                                    let with_gw = local_dir.join(format!("{sub_path}.gw"));
                                    if with_gw.exists() {
                                        return Some(with_gw);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Глобальный кэш
    search_dirs.push(global_pkg_dir());

    // Каталог компилятора (stdlib / builtins)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            search_dirs.push(exe_dir.to_path_buf());
            if let Some(p) = exe_dir.parent() {
                search_dirs.push(p.to_path_buf());
            }
        }
    }

    for base in &search_dirs {
        let candidate = base.join(imp_path);
        // 1. Точный файл (.gw, .proto, .h, .hpp)
        if candidate.is_file() {
            return Some(candidate);
        }
        // 2. Если директория -> ищем входной файл
        if candidate.is_dir() {
            if let Some(entry) = find_dir_entry(&candidate) {
                return Some(entry);
            }
        }
        // 3. Автодобавление .gw если нет расширения
        if imp_path.extension().is_none() {
            let with_gw = base.join(format!("{imp}.gw"));
            if with_gw.is_file() {
                return Some(with_gw);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dep_spec() {
        let (url, path, tag) = parse_dep_spec("github.com/alice/supermath");
        assert_eq!(url, "https://github.com/alice/supermath.git");
        assert_eq!(path, PathBuf::from("github.com/alice/supermath"));
        assert_eq!(tag, None);

        let (url2, path2, tag2) = parse_dep_spec("https://github.com/bob/engine.git@v2.1.0");
        assert_eq!(url2, "https://github.com/bob/engine.git");
        assert_eq!(path2, PathBuf::from("github.com/bob/engine"));
        assert_eq!(tag2, Some("v2.1.0".to_string()));
    }

    #[test]
    fn test_manifest_serialization() {
        let mut manifest = Manifest {
            package: Some(PackageConfig {
                name: "test_pkg".to_string(),
                version: "1.2.3".to_string(),
                entry: Some("src/main.gw".to_string()),
                authors: Some(vec!["Test <test@example.com>".to_string()]),
                description: Some("Test description".to_string()),
            }),
            dependencies: BTreeMap::new(),
        };
        manifest.dependencies.insert(
            "supermath".to_string(),
            Dependency::Simple("github.com/alice/supermath".to_string()),
        );

        let toml_str = toml::to_string_pretty(&manifest).expect("serialize");
        assert!(toml_str.contains("name = \"test_pkg\""));
        assert!(toml_str.contains("version = \"1.2.3\""));
        assert!(toml_str.contains("supermath = \"github.com/alice/supermath\""));

        let parsed: Manifest = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.package.as_ref().unwrap().name, "test_pkg");
        assert_eq!(parsed.dependencies.len(), 1);
    }
}
