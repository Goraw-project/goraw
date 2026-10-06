#!/usr/bin/env bash
# Перегенерация backend/*.gw из исходников LLVM/Clang/LLD транслятором C++→Goraw.
# Требует собранного дерева генерируемых заголовков (llvm-tblgen/clang-tblgen).
# Возобновляемый: уже успешно перегенерённые файлы (есть маркер .done) пропускаются.
set -u

ROOT="${ROOT:-/p/Goraw}"
LP="$ROOT/llvm-project"
B="${LLVM_BUILD:-/p/llvm-build}"        # build-дерево с генерируемыми .inc
GORAW="${GORAW:-$ROOT/target/release/goraw.exe}"
STATE="${STATE:-$ROOT/tools/.regen_state}"
LOG="$STATE/regen.log"
TIMEOUT="${TIMEOUT:-900}"                # пофайловый таймаут, сек
mkdir -p "$STATE"

# Генерируемые заголовки идут ПЕРВЫМИ, чтобы перекрыть устаревшие .inc в исходном include.
GEN="-I $B/include -I $B/tools/clang/include"

# group|backend_subdir|source_dir|extra_includes(через пробел; пути без пробелов)
GRPS=(
  "linker|linker|$LP/lld/COFF|-I $B/tools/lld/COFF -I $LP/lld/COFF -I $LP/lld/include -I $LP/llvm/include"
  "clang_basic|clang_basic|$LP/clang/lib/Basic|-I $LP/clang/include -I $LP/llvm/include"
  "clang_lex|clang_lex|$LP/clang/lib/Lex|-I $LP/clang/include -I $LP/llvm/include"
  "clang_parse|clang_parse|$LP/clang/lib/Parse|-I $LP/clang/include -I $LP/llvm/include"
  "instcombine|opt/instcombine|$LP/llvm/lib/Transforms/InstCombine|-I $B/lib/Transforms/InstCombine -I $LP/llvm/lib/Transforms/InstCombine -I $LP/llvm/include"
  "selectiondag|selectiondag|$LP/llvm/lib/CodeGen/SelectionDAG|-I $LP/llvm/lib/CodeGen/SelectionDAG -I $LP/llvm/include"
  "aarch64|aarch64|$LP/llvm/lib/Target/AArch64|-I $B/lib/Target/AArch64 -I $LP/llvm/lib/Target/AArch64 -I $LP/llvm/include"
  "x86|x86|$LP/llvm/lib/Target/X86|-I $B/lib/Target/X86 -I $LP/llvm/lib/Target/X86 -I $LP/llvm/include"
)

ts(){ date '+%H:%M:%S'; }
echo "=== regen start $(date) ===" >> "$LOG"

# Собираем список задач: (src_cpp, dst_gw, includes, size) по уже существующим backend/*.gw
TASKS="$STATE/tasks.tsv"
: > "$TASKS"
for entry in "${GRPS[@]}"; do
  IFS='|' read -r name sub srcdir incs <<< "$entry"
  bdir="$ROOT/backend/$sub"
  [ -d "$bdir" ] || continue
  for gw in "$bdir"/*.gw; do
    [ -e "$gw" ] || continue
    base="$(basename "$gw" .gw)"
    cpp="$srcdir/$base.cpp"
    [ -f "$cpp" ] || { echo "NOSRC	$name	$base" >> "$LOG"; continue; }
    sz=$(wc -c < "$cpp")
    printf '%s\t%s\t%s\t%s\t%s\n' "$sz" "$cpp" "$gw" "$name" "$incs" >> "$TASKS"
  done
done

TOTAL=$(wc -l < "$TASKS")
echo "задач: $TOTAL (сортировка от мелких к крупным)" | tee -a "$LOG"

MAXJOBS="${MAXJOBS:-8}"
CNT="$STATE/counter"; echo 0 > "$CNT"
BIGCNT="$STATE/bigcount"; echo 0 > "$BIGCNT"
# Взаимное исключение через atomic mkdir (работает и там, где нет flock — MSYS2/UCRT64).
LKD="$STATE/lock.d"; rmdir "$LKD" 2>/dev/null
_lock(){ local i=0; until mkdir "$LKD" 2>/dev/null; do sleep 0.03; i=$((i+1)); [ "$i" -gt 2000 ] && { rmdir "$LKD" 2>/dev/null; }; done; }
_unlock(){ rmdir "$LKD" 2>/dev/null; }
# «Тяжёлый» файл: исходник крупнее BIG_THRESH байт даёт огромный AST; таких
# разрешаем не более BIG_MAX одновременно (иначе память обваливается в пик).
BIG_THRESH="${BIG_THRESH:-400000}"
BIG_MAX="${BIG_MAX:-2}"
echo "параллельно: $MAXJOBS воркеров (гигантов >$((BIG_THRESH/1000))KB — не более $BIG_MAX)" | tee -a "$LOG"

# Обработка одного файла. Тяжёлый вызов — вне лока (параллельно);
# инкремент счётчика, mv и запись в лог — под _lock (атомарно, без гонок).
process_one(){
  local sz="$1" cpp="$2" gw="$3" name="$4" incs="$5"
  local base; base="$(basename "$gw" .gw)"
  local marker="$STATE/done/$name/$base.done"
  mkdir -p "$(dirname "$marker")"
  [ -f "$marker" ] && return 0
  local tmp="$STATE/tmp_${name}_${base}.gw"
  local err="$STATE/err_${name}_${base}.txt"
  local t0 t1 dt rc warns st idx big=0
  [ "$sz" -ge "$BIG_THRESH" ] && big=1
  [ "$big" = 1 ] && { _lock; echo $(( $(cat "$BIGCNT") + 1 )) > "$BIGCNT"; _unlock; }
  t0=$(date +%s)
  # $GEN и $incs — БЕЗ кавычек: расщепляются на отдельные токены -I и путь.
  if timeout "$TIMEOUT" "$GORAW" from-cpp "$cpp" -o "$tmp" $GEN $incs > "$err" 2>&1; then rc=0; else rc=$?; fi
  [ "$big" = 1 ] && { _lock; echo $(( $(cat "$BIGCNT") - 1 )) > "$BIGCNT"; _unlock; }
  t1=$(date +%s); dt=$((t1-t0))
  warns=$(grep -c "предупреждение" "$err" 2>/dev/null); warns=${warns:-0}
  _lock
  idx=$(( $(cat "$CNT") + 1 )); echo "$idx" > "$CNT"
  if [ -s "$tmp" ]; then
    mv -f "$tmp" "$gw"; touch "$marker"
    st="OK"; [ "$warns" -gt 0 ] && st="WARN"
    echo "[$idx/$TOTAL] $st $name/$base  ${dt}s rc=$rc warns=$warns $(wc -l < "$gw") строк" | tee -a "$LOG"
  else
    rm -f "$tmp"
    echo "[$idx/$TOTAL] FAIL $name/$base  ${dt}s rc=$rc (старый .gw сохранён)" | tee -a "$LOG"
    tail -3 "$err" >> "$LOG"
  fi
  _unlock
  rm -f "$err"
}

# Страховка по памяти: не стартуем новый воркер, если свободно < MINFREE_KB
# (защита от всплеска RAM и невыгружаемого пула ядра на файлах-гигантах).
MINFREE_KB="${MINFREE_KB:-12582912}"   # 12 ГБ
mem_free(){ awk '/^MemFree:/{print $2}' /proc/meminfo 2>/dev/null || echo 999999999; }

# Пул: мелкие файлы первыми; не более MAXJOBS одновременно И только при наличии RAM.
while IFS=$'\t' read -r sz cpp gw name incs; do
  is_big=0; [ "$sz" -ge "$BIG_THRESH" ] && is_big=1
  while :; do
    running=$(jobs -r | wc -l)
    if [ "$running" -ge "$MAXJOBS" ]; then wait -n; continue; fi
    if [ "$running" -ge 1 ] && [ "$(mem_free)" -lt "$MINFREE_KB" ]; then wait -n; continue; fi
    if [ "$is_big" = 1 ] && [ "$running" -ge 1 ] && [ "$(cat "$BIGCNT" 2>/dev/null || echo 0)" -ge "$BIG_MAX" ]; then wait -n; continue; fi
    break
  done
  process_one "$sz" "$cpp" "$gw" "$name" "$incs" &
done < <(sort -n "$TASKS")
wait

echo "=== regen done $(date) ===" >> "$LOG"
