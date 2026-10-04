# Goraw Native Backend (SelectionDAG + X86 Target + LLD-Link / LLVM-Link)

Полностью автономный бекенд компилятора Goraw, написанный на чистом Goraw без внешних зависимостей от системного LLVM, Clang или MSVC.

## Архитектура полного цикла (100% Native)

```
                       [ Goraw Source (.gw) ]
                                  │
                                  ▼ (gorawc frontend - Rust)
                          [ Goraw AST / IR ]
                                  │
                                  ▼
┌─────────────────────────────────┴─────────────────────────────────┐
│ backend/                                                           │
│                                                                    │
│  ├── selectiondag/ (26 модулей, 87,132 строк .gw)                 │
│  │     Построение графа DAG из IR, канонизация, свёртка узлов      │
│  │     (DAGCombiner), легализация типов (LegalizeTypes), операций │
│  │     (LegalizeDAG) и планирование инструкций (ScheduleDAG).      │
│  │                                                                 │
│  ├── x86/ (65 модулей, 77,180 строк .gw)                          │
│  │     Селекция инструкций x86-64 (X86ISelDAGToDAG,                │
│  │     X86ISelLowering), фреймы стека (X86FrameLowering),          │
│  │     соглашения о вызовах Win64/SysV (X86CallingConv),           │
│  │     регистры (X86RegisterInfo) и генерация листинга (X86Asm).  │
│  │                                                                 │
│  └── linker/ (21 модуль, 14,130 строк .gw)                        │
│        Нативный линковщик LLD-Link / COFF (Driver, Writer, Chunks, │
│        InputFiles, SymbolTable, Symbols, DLL, PDB, ICF) +          │
│        LLVM-Link (LinkModules, IRMover, LLVMLink).                 │
│        Формирует готовые Windows PE/COFF (.exe) без сторонних утилит│
└─────────────────────────────────┬─────────────────────────────────┘
                                  │
                                  ▼ (Ассемблерный листинг Goraw-asm)
┌─────────────────────────────────┴─────────────────────────────────┐
│ gorawas (src/asm/ - встроен в Goraw)                              │
│ Прямое машинное кодирование инструкций x86-64 через iced-x86      │
│ Генерация объектников COFF (.obj) через crate object               │
└─────────────────────────────────┬─────────────────────────────────┘
                                  │
                                  ▼ (COFF .obj)
┌─────────────────────────────────┴─────────────────────────────────┐
│ backend/linker/ (нативный Goraw LLD-Link)                          │
│ Автономный сборщик исполняемых PE-файлов (.exe) без Clang и MSVC  │
└─────────────────────────────────┬─────────────────────────────────┘
                                  │
                                  ▼
                     [ Исполняемый файл (.exe) ]
```

## Статистика кодовой базы бекенда

| Компонент | Назначение | Модулей | Строк Goraw | Размер |
|-----------|------------|---------|-------------|--------|
| **SelectionDAG** | Оптимизатор и планировщик графа | 26 | 87,132 | 3.77 МБ |
| **X86 Target** | Селекция инструкций и генератор x86-64 | 65 | 77,180 | 3.02 МБ |
| **AArch64 Target** | Селекция инструкций и кодоген ARM64 | 56 | 58,653 | 2.26 МБ |
| **Linker (LLD-Link + llvm-link)** | Компоновщик PE/COFF (.exe) и IR | 21 | 14,130 | 0.45 МБ |
| **InstCombine Optimizer** | Алгебраическая оптимизация и свертка инструкций | 16 | 35,670 | 1.31 МБ |
| **Clang Lexer & Preprocessor** | Токенизатор, препроцессор, макросы | 26 | 18,812 | 0.65 МБ |
| **Clang Parser & AST** | Синтаксический анализатор C++23 AST | 19 | 29,800 | 1.11 МБ |
| **ИТОГО** | **Автономный стек Goraw (Бекенд + Оптимизатор + C++ Фронтенд)** | **229** | **321,377** | **12.57 МБ** |

## Модульная структура (229 отдельных файлов + mod.gw)

Вся кодовая база бекенда организована по чистой модульной модели без монолитных гигантов:

1. **`backend/selectiondag/`**:
   - 26 независимых файлов (.gw)
   - Точка входа: [`backend/selectiondag/mod.gw`](file:///p:/Goraw/backend/selectiondag/mod.gw)

2. **`backend/x86/`**:
   - 65 независимых файлов (.gw)
   - Точка входа: [`backend/x86/mod.gw`](file:///p:/Goraw/backend/x86/mod.gw)

3. **`backend/aarch64/`**:
   - 56 независимых файлов (.gw)
   - Точка входа: [`backend/aarch64/mod.gw`](file:///p:/Goraw/backend/aarch64/mod.gw)

4. **`backend/linker/`**:
   - 21 независимый файл (.gw)
   - Точка входа: [`backend/linker/mod.gw`](file:///p:/Goraw/backend/linker/mod.gw)

5. **`backend/opt/instcombine/`**:
   - 16 независимых файлов (.gw)
   - Точка входа: [`backend/opt/instcombine/mod.gw`](file:///p:/Goraw/backend/opt/instcombine/mod.gw)

6. **`backend/clang_lex/`**:
   - 26 независимых файлов (.gw)
   - Точка входа: [`backend/clang_lex/mod.gw`](file:///p:/Goraw/backend/clang_lex/mod.gw)

7. **`backend/clang_parse/`**:
   - 19 независимых файлов (.gw)
   - Точка входа: [`backend/clang_parse/mod.gw`](file:///p:/Goraw/backend/clang_parse/mod.gw)

8. **Единый фасад**:
   - [`backend/mod.gw`](file:///p:/Goraw/backend/mod.gw) объединяет все подсистемы через модульные импорты.




