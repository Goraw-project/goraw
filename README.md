# Goraw

Статически типизированный системный язык, компилируемый в нативный код
через LLVM. Написан за пару вечеров «паразитируя на LLVM»: компилятор
генерирует **текстовый LLVM IR**, а `clang` превращает его в `.exe`.

Идея синтаксиса — смесь:

- **Go** — минимум ключевых слов, простые объявления функций/структур,
  цикл `for i := 0; i < n; i++`.
- **Rust** — безопасные указатели: адрес брать можно всегда, а
  разыменование сырого указателя, индексация и `int↔ptr` касты — только
  в `unsafe` (важно для драйверов/ядра).
- **Декларативный JIT** — блок `jit(...) { ... }` компилируется и
  «разворачивается на лету» в рантайме через LLVM ORC.

Плюс две вещи «под LLM»:

- **функциональная математика**: конвейер `|>` и math-builtins на
  LLVM-интринзиках;
- **ошибки сборки в машиночитаемом виде**: строгий JSON с XML-нотками
  (`--json`), чтобы модель могла их парсить и чинить.

Экосистема (три инструмента):

- **`gorawc`** — компилятор языка (`.gw` → `.exe`);
- **`gorawas`** — нативный ассемблер Goraw-asm (Intel-диалект, backend
  `iced-x86`, вывод COFF `.obj`);
- **`gorawpb`** — компилятор Protobuf (Editions) в Goraw (`.proto` → `.gw`
  со структурами и `encode_*`/`decode_*`).

Язык уже умеет **динамическую память** (куча + срезы `[]T`), поэтому на
нём самом написан, например, protobuf-рантайм.

```goraw
extern fn printf(fmt: *u8, ...) -> i32;

fn fib(n: i64) -> i64 {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> i32 {
    printf("fib(20) = %lld\n", fib(20));
    return 0;
}
```

## Сборка компилятора

Нужны **Rust** (1.97+) и **LLVM/clang** в `PATH` (проверялось на LLVM 22,
цель `x86_64-w64-windows-gnu`).

```sh
cargo build --release
# бинари: target/release/{gorawc, gorawas, gorawpb}
```

Ассемблер и protobuf:

```sh
gorawas prog.asm -o prog.obj            # Goraw-asm → COFF .obj
gorawpb schema.proto -o schema.gw       # .proto (Editions) → Goraw
```

Protobuf-пример (encode+decode, байты идентичны protoc) —
[examples/proto/](examples/proto/).

## Использование

```
gorawc <файл.gw> [опции]

  -o <путь>        имя выходного файла (.exe или .ll)
  --emit-llvm      остановиться на LLVM IR (.ll), не звать clang
  --json           диагностика в LLM-формате (JSON + XML-нотки)
  --run            запустить программу после успешной сборки
  --test           собрать и прогнать shadow-тесты (test-блоки)
  -O<n>            уровень оптимизации clang (напр. -O2)
  --keep-ll        не удалять промежуточный .ll
  --clang <путь>   путь к clang (по умолчанию из PATH)
```

Примеры:

```sh
gorawc examples/tour.gw --run          # собрать и запустить
gorawc examples/math.gw --emit-llvm    # посмотреть сгенерированный IR
gorawc broken.gw --json                # ошибки в JSON для LLM
```

## Язык

### Типы

| Категория | Типы |
|-----------|------|
| Целые | `i8 i16 i32 i64`, `u8 u16 u32 u64` |
| Плавающие | `f32 f64` |
| Прочее | `bool`, `void` |
| Указатели | `*T` (только чтение через unsafe), `*mut T` |
| Срезы | `[]T` — fat-pointer (`.ptr`, `.len`, безопасная индексация) |
| Функции | `fn(T1, T2) -> R` |
| Пользовательские | `struct`, `enum` (C-style, `-> i32`) |

Строковый литерал имеет тип `*u8` (C-строка с завершающим нулём).
`null` — нулевой указатель.

Числовые литералы подстраиваются под ожидаемый тип: в `let x: i32 = 0`
и `f64`-контексте `0` станет нужного типа. Неявных приведений между
разными типами нет — используйте `as`.

### Функции и C-interop

```goraw
extern fn printf(fmt: *u8, ...) -> i32;   // вариадики поддержаны

fn add(a: i64, b: i64) -> i64 {           // -> R необязателен (иначе void)
    return a + b;
}
```

### Переменные и управление

```goraw
let x = 10;            // неизменяемая, тип выведен (i64)
let mut sum: i64 = 0;  // изменяемая, с аннотацией
name := expr;          // Go-style короткое объявление (изменяемое)
x += 1; y <<= 2;       // составные присваивания (+= -= *= /= %= &= |= ^= <<= >>=)

if cond { ... } else { ... }
let m = if a > b { a } else { b };   // if как выражение (обе ветви обязательны)
while cond { ... }
for i := 0; i < n; i++ { ... }       // трёхчастный
for cond { ... }                     // while-форма
for { ... }                          // бесконечный; break/continue
for x in slice { ... }               // итерация по срезу/массиву
```

### Константы и глобальные переменные

```goraw
const MAX: i64 = 100;          // свёртка в компайл-тайме (арифметика, enum, cast)
const HALF: i64 = MAX / 2;     // const может ссылаться на const
static NEXT_ID: i64 = 1;       // изменяемое состояние уровня модуля
```

### Динамическая память: куча и срезы

```goraw
let p: *mut u8 = alloc(64);       // malloc; realloc/free/mem_copy/mem_set
let xs: []i64 = make_slice(p as *mut i64, 8);
for x in xs { /* ... */ }          // xs.len, xs[i] — безопасны
free(p);
```

На куче и срезах на самом Goraw пишутся `Vec`/`Bytes` (см.
[examples/bytes.gw](examples/bytes.gw), [examples/slices.gw](examples/slices.gw)).
Ещё builtins: `sizeof(T)`, `zeroed()`, `f32_bits`/`f64_bits` и обратные.

### Массивы и строки

```goraw
let a: [4]i64 = [10, 20, 30, 40];    // фиксированный размер, семантика значения
let n = a.len;                        // константа; a[i] с проверкой границ
sum_slice(a);                         // [N]T авто-коэрсится в []T

let s: str = "goraw";                 // str == []u8; s.len, s[i], for c in s
```

### Перечисления

```goraw
enum Color { Red, Green, Blue }             // 0, 1, 2
enum Status { Ok = 0, NotFound = 404 }      // явные значения
let c: Color = Color::Green;                // доступ через ::
```

### Модули

```goraw
import "mod_math.gw";   // подключает объявления другого файла
```

Резолвится компилятором (рекурсивно, с дедупом); диагностики остаются
привязаны к исходным файлам. Namespacing/пакеты — в планах.

### Структуры

```goraw
struct Vec2 { x: f64, y: f64 }

fn dot(a: Vec2, b: Vec2) -> f64 {
    return a.x * b.x + a.y * b.y;
}

let v = Vec2 { x: 1.5, y: 2.0 };   // литерал, поля обязательны все
```

Структуры передаются и возвращаются по значению.

Методы: `self` — безопасная ссылка (`*mut Type`), доступ к полям без `unsafe`:

```goraw
fn Vec2::len2(self) -> f64 { return self.x * self.x + self.y * self.y; }
fn Counter::inc(self) { self.value += 1; }   // мутация через self

let d = v.len2();   // вызов метода
```

### Safe / unsafe и указатели

Взятие адреса безопасно. Разыменование, индексация указателя и касты
`int↔ptr` требуют `unsafe` — это и есть граница «безопасно/небезопасно»:

```goraw
let mut cell: i64 = 41;
let p: *mut i64 = &mut cell;   // безопасно
unsafe {
    *p = *p + 1;               // разыменование — только в unsafe
}
```

Функцию целиком можно пометить `unsafe fn`.

### Функциональная математика

Конвейер `|>` подставляет левое значение первым аргументом правого вызова
(как в Elixir) и связывает слабее арифметики:

```goraw
// a*a + b*b |> sqrt  ==  sqrt(a*a + b*b)
fn hypot(a: f64, b: f64) -> f64 { return a*a + b*b |> sqrt; }

let r = -15.9 |> abs |> sqrt |> ceil;   // цепочка
```

Встроенные функции (ложатся на `llvm.*`-интринзики):

- float: `sqrt sin cos exp exp2 log log2 log10 floor ceil round trunc
  fabs pow fma`
- число (float/int): `min max abs clamp`

### Инлайн-ассемблер

Intel-синтаксис (`masm`/`nasm`) транслируется в LLVM `inteldialect`.
Регистры-скретчи (`eax`, `rax`, …) автоматически помечаются как
затираемые. Доступно только в `unsafe`.

```goraw
unsafe fn add100(value: u32) -> u32 {
    let mut output: u32 = 0;
    asm("masm", inputs: [value], outputs: [output]) {
        mov eax, value      ; комментарии после ';' игнорируются
        add eax, 100
        mov output, eax
    };
    return output;
}
```

### JIT-блоки (рантайм-специализация)

`jit(...)` компилирует внутреннюю функцию **в рантайме**, вкомпилируя
захваченные переменные как константы — «разворачивание на лету» через
LLVM ORC. Значение блока — функция-указатель.

```goraw
let base: i32 = 10;
let scale = jit(captures: [base]) {
    fn execute(a: i32, b: i32) -> i32 {
        return (a + b) * base;   // base вкомпилируется как константа 10
    }
};
let r = scale(5, 3);   // = 80, вызов уже нативной специализации
```

Разные значения захвата дают разный машинный код на лету. В jit-блоке
доступны параметры, захваты, математика и `extern` C-функции
(пользовательские функции пока нельзя — они не резолвятся в рантайме).

Требование рантайма: `LLVM-C.dll` должна быть в `PATH` (каталог
`LLVM\bin`). C-рантайм линкуется в `.exe` автоматически и только если
программа использует `jit`.

## Встроенные тесты (Shadow Tests)

`test`-блоки живут рядом с кодом, но **вырезаются из релиза**. Под `--test`
каждый компилируется и прогоняется; `assert` допустим только в них.

```goraw
fn add(a: i64, b: i64) -> i64 { return a + b; }

test "сложение" {
    assert add(2, 3) == 5;
    assert add(-1, 1) == 0;
}
```

```sh
gorawc prog.gw --test    # [ ok ]/[FAIL] по каждому + exit-код для CI
gorawc prog.gw --run     # релиз: тесты не попадают в бинарь
```

## Диагностика для LLM

С флагом `--json` ошибки печатаются как строгий JSON, где у каждой
диагностики есть код (`E0030`), позиция, `hint` и вложенный XML-фрагмент
`<explain>` с контекстом строки и кареткой. За один проход собирается
несколько ошибок.

```json
{
  "schema": "goraw.diagnostics/v1",
  "ok": false,
  "errors": 1,
  "diagnostics": [
    {
      "severity": "error",
      "code": "E0030",
      "message": "разыменование сырого указателя требует `unsafe`",
      "line": 6, "col": 5,
      "hint": "оберните код в `unsafe { ... }` или пометьте функцию `unsafe fn`",
      "explain": "<explain code=\"E0030\" ...>...</explain>"
    }
  ]
}
```

## Архитектура

```
src/
  lexer.rs    — исходник -> токены (со спанами)
  ast.rs      — синтаксическое дерево
  parser.rs   — рекурсивный спуск + Pratt для выражений
  types.rs    — система типов и сбор сигнатур (1-й проход)
  codegen.rs  — семантика + генерация LLVM IR (2-й проход)
  diag.rs     — диагностики: человекочитаемо и в LLM-JSON (мульти-файл)
  main.rs     — CLI-драйвер gorawc, резолвинг import, вызов clang
  lib.rs      — общая библиотека для всех бинарей
  asm/        — ассемблер Goraw-asm (gorawas): парсер + iced-x86 + object
  proto/      — protobuf Editions (gorawpb): парсер .proto, дескрипторы,
                резолвинг features, кодоген в Goraw (encode/decode)
  bin/        — gorawas.rs, gorawpb.rs
runtime/
  goraw_jit.c — рантайм JIT-специализации на LLVM-C ORC
```

Пайплайн: `.gw` → лексер → парсер → сбор типов → кодоген (LLVM IR) →
`clang --target=x86_64-w64-windows-gnu` → `.exe`.

## Примеры

- [`examples/tour.gw`](examples/tour.gw) — обзор: рекурсия, структуры,
  цикл, unsafe.
- [`examples/math.gw`](examples/math.gw) — конвейер `|>` и математика.
- [`examples/asm.gw`](examples/asm.gw) — инлайн-ассемблер.
- [`examples/jit.gw`](examples/jit.gw) — JIT-специализация.
- [`examples/bytes.gw`](examples/bytes.gw) / [`examples/slices.gw`](examples/slices.gw)
  — куча, срезы, protobuf varint на Goraw.
- [`examples/enums.gw`](examples/enums.gw),
  [`examples/modules_demo.gw`](examples/modules_demo.gw) — enum, import.
- [`examples/arrays.gw`](examples/arrays.gw),
  [`examples/strings.gw`](examples/strings.gw),
  [`examples/methods.gw`](examples/methods.gw) — массивы, `str`, методы.
- [`examples/consts.gw`](examples/consts.gw),
  [`examples/statics.gw`](examples/statics.gw) — `const`/`static`.
- [`examples/tests_demo.gw`](examples/tests_demo.gw) — shadow-тесты (`--test`).
- [`examples/proto/`](examples/proto/) — protobuf через `gorawpb`.

## Ограничения и что дальше

Это игрушечный, но настоящий компилятор. Есть: куча, срезы `[]T`
(bounds-checked), массивы `[N]T`, `str`, enum, `const`/`static`, методы
структур, if-выражения, модули (`import`), shadow-тесты, protobuf
(encode+decode). Пока нет: обобщений, замыканий (кроме jit), сборки мусора,
namespacing модулей, `map`/`oneof` в protobuf. Вызов пользовательских
функций из jit-блока не поддержан. Дорожная карта — [docs/ROADMAP.md](docs/ROADMAP.md).
