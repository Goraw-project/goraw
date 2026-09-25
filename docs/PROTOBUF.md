# Protobuf (Editions & PB2) в Goraw (`gorawpb`)

`gorawpb` — компилятор схем Protobuf в нативный код Goraw. Он генерирует типизированные структуры данных и легковесные методы сериализации (`encode_*`) и десериализации (`decode_*`), **100% совместимые по wire-формату с официальным Google `protoc`**.

---

## 1. Возможности и поддерживаемые типы

Рантайм и генератор `gorawpb` поддерживают полный набор возможностей Protobuf v2 и современных Editions:

| Категория | Поддерживаемые конструкции | Wire Type |
|---|---|---|
| Целые примитивы | `int32`, `int64`, `uint32`, `uint64`, `bool`, `enum` | Type 0 (Varint) |
| 64-битные фикс. | `fixed64`, `sfixed64`, `double` | Type 1 (64-bit) |
| Длина-данные | `string`, `bytes`, вложенные сообщения | Type 2 (Length-delimited) |
| 32-битные фикс. | `fixed32`, `sfixed32`, `float` | Type 5 (32-bit) |
| Коллекции | `repeated string`, `repeated <message>`, `packed repeated` | Type 2 (Length-delimited) |
| Словари | `map<K, V>` (любые ключи и значения) | Type 2 (Submessage entry) |
| Варианты | `oneof` (разреженные поля с селектором) | Wire-level tag |

---

## 2. Использование генератора `gorawpb`

Команда генерации:
```bash
.\box.cmd gorawpb schema.proto -o schema.gw
```

### Пример proto-схемы
Файл `user.proto`:
```protobuf
syntax = "proto2";

message UserProfile {
    required int64 id = 1;
    required string username = 2;
    repeated string tags = 3;
    map<string, string> attributes = 4;
}
```

### Сгенерированный код Goraw
Генератор создает структуры Goraw и чистые функции кодирования/декодирования:
```goraw
// Сгенерировано gorawpb
struct UserProfile {
    id: i64,
    username: str,
    tags: []str,
    attributes: Map_str_str,
}

fn encode_UserProfile(msg: &UserProfile, buf: *mut ByteBuffer) { ... }
fn decode_UserProfile(buf: &ByteBuffer) -> UserProfile { ... }
```

---

## 3. Схема сериализации `map<K, V>` и `oneof`

1. **`map<K, V>`**:
   По стандарту Google Protobuf, запись карты в wire-формате эквивалентна повторяющемуся сообщению записи:
   ```protobuf
   message Entry {
       optional Key key = 1;
       optional Value value = 2;
   }
   ```
   `gorawpb` автоматически сериализует пары ключ-значение в соответствии с этим форматом и восстанавливает их при десериализации.

2. **`oneof`**:
   Поля `oneof` размещаются в структуре совместно с полем-дискриминатором `which_field`. При записи кодируется только активное поле, при чтении обновляется соответствующий вариант.

---

## 4. Верификация совместимости с `protoc`

Корректность генератора доказана тестом в [examples/proto/](file:///P:/Goraw/examples/proto/):
- Байт-в-байт совпадение сгенерированного бинарного потока с выводом канонического `protoc --encode`.
- Модульный тест компилятора ([src/proto.rs](file:///P:/Goraw/src/proto.rs)) автоматически прогоняется при каждой команде `box test`.
