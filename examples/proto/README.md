# gorawpb — protobuf (Editions) → Goraw

Демонстрация цепочки: `.proto` → **gorawpb** → Goraw → **gorawc** → `.exe` →
байты, идентичные wire-формату protobuf.

Пока в языке нет модулей, драйвер (`*_main.gw`) склеивается с сгенерированным
кодом в один файл.

```sh
# 1) схема -> Goraw (структуры + encode_*)
gorawpb examples/proto/demo.proto -o demo.gen.gw

# 2) склеить с драйвером и собрать
cat demo.gen.gw examples/proto/demo_main.gw > demo.combined.gw
gorawc demo.combined.gw --run
```

Проверенные результаты (байт-в-байт с protoc):

- `demo.proto` — `Point{x=150, label="hi"}` → `08 96 01 1A 02 68 69`
- `rich.proto` — int64/sint32(zigzag)/bool/repeated-packed/вложенное
  сообщение/fixed32 → `08 01 10 01 18 01 22 03 01 AC 02 2A 02 08 07 35 04 03 02 01`

Резолвинг Editions-features работает: edition 2023 → explicit presence
(hasbits) и packed repeated по умолчанию.

Статус: PB1 (encode). Дальше — decode, map/oneof, JSON, кодоген для Rust,
модули (чтобы не склеивать вручную). См. `docs/ROADMAP.md` Эпик 8.
