# Goraw Box — Изоляция и контейнеризация сборки

Контейнерный раннер `box` (`box.cmd` и `box.ps1`) обеспечивает строго контролируемую среду выполнения для компилятора `gorawc`, ассемблера `gorawas`, генератора protobuf `gorawpb`, а также для всех тестов и пользовательских бинарников.

Главная цель контейнера — **гарантия абсолютной защиты хост-системы от исчерпания памяти (OOM cascades), утечек ресурсов и зависаний**, с предоставлением подробной низкоуровневой телеметрии ядра Windows.

---

## 1. Архитектура изоляции

Среда `box` построена на связке двух компонентов ядра Windows:

1. **Process Governor (`tools/procgov/procgov64.exe`)**:
   - Утилита низкоуровневого управления процессами через Windows Job Objects API.
   - Запускается с флагами `-m 16G -r -q -- <команда>`:
     - `-m 16G` — жесткий потолок виртуальной и рабочей памяти (Working Set + Commit Limit) в **16 Гигабайт**. При превышении лимита процесс немедленно прерывается операционной системой без возможности вызвать своппинг или BSOD хоста.
     - `-r` (`--recursive`) — ограничение распространяется на всю ветку дочерних процессов, порожденных компилятором, линкером `clang`, `lld` и исполняемыми тестами.
     - `-q` — подавление служебного вывода самого procgov для прозрачной передачи потоков stdin/stdout/stderr целевого процесса.

2. **Модуль телеметрии ядра (`tools/procgov/GorawJobHelper.dll`)**:
   - Собственная C# P/Invoke библиотека ([GorawJobHelper.cs](file:///P:/Goraw/tools/procgov/GorawJobHelper.cs)), обращающаяся к Win32 API:
     - `CreateJobObjectW`
     - `QueryInformationJobObject` (`JobObjectBasicAccountingInformation`, `JobObjectExtendedLimitInformation`)
     - `AssignProcessToJobObject`
   - Извлекает аппаратные счетчики ядра в реальном времени:
     - **Wall Clock Time**: фактическое время выполнения (мс / с).
     - **User CPU Time & Kernel CPU Time**: чистое процессорное время, разделенное на пространство пользователя и ядра.
     - **Peak Memory**: пиковое потребление физической памяти (Peak Working Set) всей группой процессов.
     - **Process Count**: суммарное число процессов, созданных внутри контейнера (например, cargo -> rustc -> clang -> lld).

---

## 2. Команды и использование

Раннер вызывается как через PowerShell (`.\box.ps1`), так и через стандартную командную строку Windows / CI (`.\box.cmd`).

### Быстрые команды

| Команда | Описание |
|---|---|
| `.\box.cmd test` | Запуск полного набора unit-тестов проекта (`cargo test`) под лимитом 16 ГБ |
| `.\box.cmd build [args]` | Сборка компилятора (`cargo build`) |
| `.\box.cmd check` | Быстрая проверка типов и синтаксиса (`cargo check`) |
| `.\box.cmd run [args]` | Запуск компилятора через `cargo run` |
| `.\box.cmd gorawc <args>` | Запуск скомпилированного бинарника `gorawc` |
| `.\box.cmd gorawas <args>` | Запуск ассемблера `gorawas` (автоматически пересобирает при отсутствии) |
| `.\box.cmd gorawpb <args>` | Запуск компилятора схем `gorawpb` |
| `.\box.cmd <cmd> [args]` | Запуск **любой** произвольной команды ОС в изолированном контейнере |

### Примеры запуска

```bash
# 1. Прогон всех тестов проекта с телеметрией
.\box.cmd test

# 2. Сборка Goraw программы с ассемблерным модулем
.\box.cmd gorawc examples\native_greet_main.gw examples\native_greet.asm --run

# 3. Ассемблирование чистого Goraw-asm
.\box.cmd gorawas examples\native_hello.asm -o examples\native_hello.obj

# 4. Проверка строгого режима shadow-тестов
.\box.cmd gorawc examples\shadow_strict_demo.gw --shadow=strict --test

# 5. Изолированный запуск сторонней команды
.\box.cmd clang --target=x86_64-pc-windows-gnu examples\native_hello.obj -o hello.exe
```

---

## 3. Вывод телеметрии

После завершения выполнения любой команды (успешного или аварийного) `box` выводит форматированную ASCII-плашку телеметрии и **сохраняет точный код возврата (exit code)** целевого процесса:

```text
┌────────────────────────────────────────────────────────┐
│                Goraw Container Telemetry               │
├────────────────────────────────────────────────────────┤
│ Status:       SUCCESS (Exit Code: 0)                   │
│ Wall Clock:   446 ms                                   │
│ CPU Time:     0,12 s (User: 0,02s | Kernel: 0,11s)     │
│ Peak Memory:  55,76 MB / 16,384 MB [0,34%]             │
│ Processes:    5 spawned in container                   │
│ Memory Limit: 16.00 GB (Enforced via procgov64)        │
└────────────────────────────────────────────────────────┘
```

При аварийном завершении:
```text
┌────────────────────────────────────────────────────────┐
│                Goraw Container Telemetry               │
├────────────────────────────────────────────────────────┤
│ Status:       FAILED (Exit Code: 1)                    │
│ Wall Clock:   270 ms                                   │
│ CPU Time:     0,00 s (User: 0,00s | Kernel: 0,00s)     │
│ Peak Memory:  0,85 MB / 16,384 MB [0,01%]              │
│ Processes:    1 spawned in container                   │
│ Memory Limit: 16.00 GB (Enforced via procgov64)        │
└────────────────────────────────────────────────────────┘
```

---

## 4. Конфигурация Cargo Runner

Для автоматического применения контейнера к стандартным вызовам `cargo test` и `cargo run` в корне репозитория настроен [.cargo/config.toml](file:///P:/Goraw/.cargo/config.toml):

```toml
[target.x86_64-pc-windows-msvc]
runner = ["P:\\Goraw\\tools\\procgov\\procgov64.exe", "-m", "16G", "-r", "-q", "--"]

[target.x86_64-pc-windows-gnu]
runner = ["P:\\Goraw\\tools\\procgov\\procgov64.exe", "-m", "16G", "-r", "-q", "--"]
```

Это гарантирует, что даже при прямом вызове `cargo test` из IDE или скрипта тесты запускаются с защитой от OOM.
