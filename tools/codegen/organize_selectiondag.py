import os
import re
from pathlib import Path

src_dir = Path(r"p:\Goraw\scratch")
out_dir = Path(r"p:\Goraw\selectiondag")
out_dir.mkdir(parents=True, exist_ok=True)

modules = [
    "DAGCombiner",
    "FastISel",
    "FunctionLoweringInfo",
    "InstrEmitter",
    "LegalizeDAG",
    "LegalizeFloatTypes",
    "LegalizeIntegerTypes",
    "LegalizeTypes",
    "LegalizeTypesGeneric",
    "LegalizeVectorOps",
    "LegalizeVectorTypes",
    "ResourcePriorityQueue",
    "ScheduleDAGFast",
    "ScheduleDAGRRList",
    "ScheduleDAGSDNodes",
    "ScheduleDAGVLIW",
    "SDNodeInfo",
    "SelectionDAG",
    "SelectionDAGAddressAnalysis",
    "SelectionDAGBuilder",
    "SelectionDAGDumper",
    "SelectionDAGISel",
    "SelectionDAGPrinter",
    "SelectionDAGTargetInfo",
    "StatepointLowering",
    "TargetLowering",
]

def clean_line_assert(line):
    prefix = "(((!!("
    idx = line.find(prefix)
    if idx != -1:
        wassert_marker = " || (_wassert("
        wassert_idx = line.find(wassert_marker, idx)
        if wassert_idx != -1:
            cond_end = wassert_idx
            if line[cond_end-2:cond_end] == "))":
                cond_raw = line[idx + len(prefix) : cond_end - 2].strip()
            elif line[cond_end-1:cond_end] == ")":
                cond_raw = line[idx + len(prefix) : cond_end - 1].strip()
            else:
                cond_raw = line[idx + len(prefix) : cond_end].strip()
                if cond_raw.endswith("))"):
                    cond_raw = cond_raw[:-2].strip()
                elif cond_raw.endswith(")"):
                    cond_raw = cond_raw[:-1].strip()

            indent = line[:idx]
            msg_m = re.search(r"^(.*?)\s*&&\s*(\"(?:\\.|[^\"])*\")$", cond_raw)
            if msg_m:
                real_cond = msg_m.group(1).strip()
                msg = msg_m.group(2).strip()
                return f"{indent}assert!({real_cond}, {msg});"
            else:
                return f"{indent}assert!({cond_raw});"

    # Handle remaining standalone _wassert calls if any
    if "_wassert(" in line:
        m = re.search(r'(\s*)_wassert\(\s*"(.*?)"\s*,\s*".*?"\s*,\s*\d+\s*\);', line)
        if m:
            indent = m.group(1)
            msg_or_cond = m.group(2)
            # unescape \"
            msg_or_cond = msg_or_cond.replace(r'\"', '"')
            if ' && "' in msg_or_cond:
                parts = msg_or_cond.split(' && "', 1)
                cond_str = parts[0]
                msg_str = parts[1].rstrip('"')
                return f'{indent}assert!({cond_str}, "{msg_str}");'
            return f'{indent}assert!(false, "{msg_or_cond}");'

    return line

def clean_file_content(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    lines = content.splitlines()
    cleaned_lines = []
    
    # Skip any leading boilerplate banner
    in_banner = True
    for line in lines:
        stripped = line.strip()
        if in_banner:
            if stripped.startswith("// ====") or \
               "Автоматически транслировано" in stripped or \
               "Исходный файл:" in stripped or \
               "Fully transpiled" in stripped or \
               "LLVM SelectionDAG Monolith" in stripped:
                continue
            if stripped == "":
                continue
            in_banner = False

        # Clean assert macros
        clean_line = clean_line_assert(line)
        cleaned_lines.append(clean_line)

    return "\n".join(cleaned_lines)

monolith_parts = []

for mod in modules:
    if mod == "SelectionDAGAddressAnalysis":
        src_path = src_dir / "SelectionDAGAddressAnalysis_clean.gw"
        if not src_path.exists():
            src_path = src_dir / f"real_{mod}.gw"
    else:
        src_path = src_dir / f"real_{mod}.gw"

    if not src_path.exists():
        print(f"[MISSING] {src_path}")
        continue

    cleaned = clean_file_content(src_path)
    out_mod_path = out_dir / f"{mod}.gw"
    with open(out_mod_path, 'w', encoding='utf-8') as f:
        f.write(cleaned + "\n")

    line_count = len(cleaned.splitlines())
    kb = round(len(cleaned.encode('utf-8')) / 1024, 1)
    print(f"[OK] {mod}.gw ({line_count} lines, {kb} KB)")

    monolith_parts.append(f"import \"{mod}.gw\";")

# Write mod.gw
mod_text = "// SelectionDAG Module Entry Point\n\n" + "\n".join(monolith_parts) + "\n"
mod_out = out_dir / "mod.gw"
with open(mod_out, 'w', encoding='utf-8') as f:
    f.write(mod_text)

print(f"\n[DONE] Modular entry point written to {mod_out} ({len(modules)} modules)")

