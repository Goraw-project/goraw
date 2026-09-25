# gorawpb — protobuf (Editions) → Goraw

Демонстрация цепочки: `.proto` → **gorawpb** → Goraw → **gorawc** → `.exe` →
байты, идентичные wire-формату protobuf.

Благодаря поддержке `import` в Goraw, драйвер может напрямую импортировать сгенерированный файл:

```sh
# 1) компиляция схемы -> Goraw
gorawpb examples/proto/pb2_all.proto -o examples/proto/pb2_all.gw

# 2) сборка и запуск
gorawc --run examples/proto/pb2_main.gw
```

Проверенные результаты (байт-в-байт со спецификацией protobuf):

- `demo.proto` — `Point{x=150, label="hi"}` → `08 96 01 1A 02 68 69`
- `rich.proto` — int64/sint32(zigzag)/bool/repeated-packed/вложенное
  сообщение/fixed32 → `08 01 10 01 18 01 22 03 01 AC 02 2A 02 08 07 35 04 03 02 01`
- `pb2_all.proto` — `map<string, int32>`, `oneof payload`, `repeated string`, `repeated SubItem` (полный roundtrip encode + decode)

Резолвинг Editions-features работает: edition 2023 → explicit presence
(hasbits) и packed repeated по умолчанию.

Статус: **PB2 завершён** (encode + decode для скаляров, сообщений, `map<K,V>`, `oneof`, `repeated string/msg`). См. `docs/ROADMAP.md`.
