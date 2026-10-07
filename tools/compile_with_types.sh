#!/usr/bin/env bash
# Замер «compile-rate» backend с модулем типов Фазы 2.
# Для каждого <группа>/<файл>: генерит модуль типов (--emit-types) + перевод функций,
# объединяет через `import "types.gw";`, компилирует и считает ошибки.
#
# Требует собранного дерева генерируемых заголовков LLVM (см. BOOTSTRAP.md).
# Использование:  bash tools/compile_with_types.sh [группа/Файл.cpp ...]
#   без аргументов — прогоняет встроенный образец по всем группам.
set -u

ROOT="${ROOT:-/p/Goraw}"
LP="$ROOT/llvm-project"
B="${LLVM_BUILD:-/p/llvm-build}"
GORAW="${GORAW:-$ROOT/target/release/goraw.exe}"
GORAWC="${GORAWC:-$ROOT/target/release/gorawc.exe}"
WORK="${WORK:-$ROOT/tools/.cwt}"
TIMEOUT="${TIMEOUT:-300}"
GEN="-I $B/include -I $B/tools/clang/include"
LLV="-I $LP/llvm/include"
mkdir -p "$WORK"

# Путь к исходнику и include-набор по имени группы.
src_dir() { case "$1" in
  clang_basic) echo "$LP/clang/lib/Basic";; clang_lex) echo "$LP/clang/lib/Lex";;
  clang_parse) echo "$LP/clang/lib/Parse";; aarch64) echo "$LP/llvm/lib/Target/AArch64";;
  x86) echo "$LP/llvm/lib/Target/X86";; selectiondag) echo "$LP/llvm/lib/CodeGen/SelectionDAG";;
  instcombine) echo "$LP/llvm/lib/Transforms/InstCombine";; linker) echo "$LP/lld/COFF";;
esac; }
inc_for() { case "$1" in
  clang_*) echo "-I $LP/clang/include $LLV";;
  aarch64) echo "-I $LP/llvm/lib/Target/AArch64 $LLV";;
  x86) echo "-I $LP/llvm/lib/Target/X86 $LLV";;
  selectiondag) echo "-I $LP/llvm/lib/CodeGen/SelectionDAG $LLV";;
  instcombine) echo "-I $LP/llvm/lib/Transforms/InstCombine $LLV";;
  linker) echo "-I $B/tools/lld/COFF -I $LP/lld/COFF -I $LP/lld/include $LLV";;
esac; }

measure() {
  local grp="$1" base="$2"
  local cpp; cpp="$(src_dir "$grp")/$base.cpp"
  [ -f "$cpp" ] || { echo "$grp/$base: НЕТ исходника"; return; }
  local d="$WORK/$grp-$base"; mkdir -p "$d"; local ic; ic="$(inc_for "$grp")"
  timeout "$TIMEOUT" "$GORAW" from-cpp "$cpp" --emit-types -o "$d/types.gw" $GEN $ic >/dev/null 2>&1
  timeout "$TIMEOUT" "$GORAW" from-cpp "$cpp" -o "$d/fns.gw" $GEN $ic >/dev/null 2>&1
  printf 'import "types.gw";\n\n' | cat - "$d/fns.gw" > "$d/combined.gw"
  local tn cn
  tn=$("$GORAWC" "$d/types.gw" --emit-llvm -o /dev/null 2>&1 | grep -cE '^error\[')
  cn=$("$GORAWC" "$d/combined.gw" --emit-llvm -o /dev/null 2>&1 | grep -cE '^error\[')
  local ok="нет"; [ "$cn" = "0" ] && ok="ДА"
  echo "$grp/$base: types=$tn combined=$cn компилится=$ok"
}

if [ "$#" -gt 0 ]; then
  for arg in "$@"; do
    grp="${arg%%/*}"; file="${arg#*/}"; base="$(basename "$file" .cpp)"
    measure "$grp" "$base"
  done
else
  # встроенный образец (по одному файлу на группу)
  measure clang_basic   SourceLocation
  measure clang_lex     PPCallbacks
  measure clang_parse   ParseAST
  measure aarch64       AArch64SMEAttributes
  measure x86           X86MachineFunctionInfo
  measure selectiondag  SelectionDAGTargetInfo
  measure instcombine   InstCombineNegator
  measure linker        MarkLive
fi
