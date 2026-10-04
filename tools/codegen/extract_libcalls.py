import re
from pathlib import Path

content = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\IR\RuntimeLibcalls.td").read_text(encoding="utf-8")
content = re.sub(r"//.*", "", content)

# Look for foreach loops and defs
# e.g.:
# foreach IntTy = ["I16", "I32", "I64", "I128"] in {
#   def SHL_#IntTy : RuntimeLibcall;
# }

# Let's find all `def ... : RuntimeLibcall;` or foreach loops containing RuntimeLibcall
lines = content.splitlines()
libcalls = []

# Simple expansion of foreach loops in tablegen
i = 0
while i < len(lines):
    line = lines[i].strip()
    m_foreach = re.match(r'foreach\s+([A-Za-z0-9_]+)\s*=\s*\[([^\]]+)\]\s*in\s*\{', line)
    if m_foreach:
        var_name = m_foreach.group(1)
        vals = [v.strip().strip('"') for v in m_foreach.group(2).split(',') if v.strip()]
        # collect block
        block_lines = []
        depth = 1
        i += 1
        while i < len(lines) and depth > 0:
            if '{' in lines[i]: depth += lines[i].count('{')
            if '}' in lines[i]: depth -= lines[i].count('}')
            if depth > 0:
                block_lines.append(lines[i])
            i += 1
        
        for val in vals:
            for bl in block_lines:
                bl_sub = bl.replace('#' + var_name, val).replace(var_name, val)
                m_def = re.match(r'\s*def\s+([A-Za-z0-9_]+)\s*:\s*RuntimeLibcall\b', bl_sub)
                if m_def:
                    libcalls.append(m_def.group(1))
        continue
        
    m_def = re.match(r'\s*def\s+([A-Za-z0-9_]+)\s*:\s*RuntimeLibcall\b', line)
    if m_def:
        libcalls.append(m_def.group(1))
    i += 1

print(f"Extracted {len(libcalls)} RuntimeLibcall entries")
print("First 10:", libcalls[:10])
print("Last 10:", libcalls[-10:])

# Families
families = []
for m in re.finditer(r'def\s*:\s*RuntimeLibcallFamily\s*<\s*"([^"]+)"', content):
    families.append(m.group(1))

print(f"Extracted {len(families)} families:", families)
