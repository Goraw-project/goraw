#!/usr/bin/env python3
"""
AST-Aware Incremental Patcher for Goraw Transpiled Code.
Fixes syntactic anomalies from C++ AST translation across backend/:
1. !expr.len == 0 -> !expr.is_empty()
2. expr.operator bool() -> expr
3. Stray leading/trailing/duplicate commas: `(, `, `, )`, `,,`
4. Redundant self-constructor struct initializers: `Type { Type }` -> `Type {}`
5. StringRef / Variable pseudo-calls: `r.append(a())` -> `r.append(a)`
"""

import os
import re
import sys

def patch_content(text: str) -> tuple[str, int]:
    changes = 0
    original = text

    # 1. Fix !expr.len == 0 -> !expr.is_empty()
    # e.g., `!r.len == 0` -> `!r.is_empty()`
    # e.g., `!G.GetDbgValues(&mut Node).len == 0` -> `!G.GetDbgValues(&mut Node).is_empty()`
    def repl_not_empty(m):
        expr = m.group(1).strip()
        return f"!{expr}.is_empty()"
    
    text, n = re.subn(r'!\s*([a-zA-Z0-9_\.\->\(\)&mut\s]+?)\.len\s*==\s*0\b', repl_not_empty, text)
    changes += n

    # Also expr.len == 0 when not preceded by !
    def repl_is_empty(m):
        prefix = m.group(1)
        expr = m.group(2).strip()
        if prefix.strip().endswith('!'):
            return m.group(0) # already handled
        return f"{prefix}{expr}.is_empty()"
    
    # 2. Fix .operator bool() -> remove it
    # e.g., `VerboseDAGDumping.operator bool()` -> `VerboseDAGDumping`
    text, n = re.subn(r'\.operator\s+bool\(\)', '', text)
    changes += n

    # 3. Fix Type { Type } -> Type {}
    # e.g., `ObjNameSym { ObjNameSym }` -> `ObjNameSym {}`
    text, n = re.subn(r'\b([A-Za-z_][A-Za-z0-9_]*)\s*\{\s*\1\s*\}', r'\1 {}', text)
    changes += n

    # 4. Fix stray commas and empty slots in function calls in a loop until clean:
    while True:
        prev = text
        text = re.sub(r'\(\s*,+\s*', '(', text)
        text = re.sub(r',\s*,+', ',', text)
        text = re.sub(r',\s*\)', ')', text)
        text = re.sub(r'\(\s*\(\)\s*,\s*', '(', text)
        text = re.sub(r',\s*\(\)\s*\)', ')', text)
        text = re.sub(r',\s*\(\)\s*,', ',', text)
        text = re.sub(r'\(\s*\(\)\s*\)', '()', text)
        if text == prev:
            break
        changes += 1

    # 5. Fix specific known artifact: `r(join(` -> `r.append(join(`
    text, n = re.subn(r'\br\(join\(', 'r.append(join(', text)
    changes += n

    # 6. Specific quote fix in PDB.gw / Linker_monolith: `a(, '"');` -> `a.split(&mut s, '"');`
    text, n = re.subn(r'\ba\(\s*,\s*[\'"]"[\'"]\);', 'a.split(&mut s, \'"\');', text)
    changes += n
    text, n = re.subn(r'\br\.append\(join\(\s*,\s*"\\"\\""\)\);', 'r.append(join(s, "\\"\\""));', text)
    changes += n
    text, n = re.subn(r'\br\.append\(a\(\)\);', 'r.append(a);', text)
    changes += n

    return text, changes

def main():
    backend_dir = r"p:\Goraw\backend"
    if len(sys.argv) > 1:
        backend_dir = sys.argv[1]

    print(f"=== Scanning and Patching Goraw Backend in: {backend_dir} ===")
    
    total_files_patched = 0
    total_changes = 0

    for root, dirs, files in os.walk(backend_dir):
        for f in files:
            if not f.endswith(".gw"):
                continue
            fpath = os.path.join(root, f)
            with open(fpath, "r", encoding="utf-8", errors="replace") as fp:
                content = fp.read()
            
            new_content, n_changes = patch_content(content)
            if n_changes > 0:
                with open(fpath, "w", encoding="utf-8") as fp:
                    fp.write(new_content)
                total_files_patched += 1
                total_changes += n_changes
                print(f"[PATCHED] {os.path.relpath(fpath, backend_dir)}: {n_changes} fixes applied")

    print("\n" + "=" * 60)
    print(f"Done! Patched {total_files_patched} files with {total_changes} total AST fixes.")
    print("=" * 60)

if __name__ == "__main__":
    main()
