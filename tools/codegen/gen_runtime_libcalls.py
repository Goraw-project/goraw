import re
from pathlib import Path

def generate_runtime_libcalls():
    content = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\IR\RuntimeLibcalls.td").read_text(encoding="utf-8")
    content = re.sub(r"//.*", "", content)

    lines_raw = content.splitlines()
    libcalls = []

    i = 0
    while i < len(lines_raw):
        line = lines_raw[i].strip()
        m_foreach = re.match(r'foreach\s+([A-Za-z0-9_]+)\s*=\s*\[([^\]]+)\]\s*in\s*\{', line)
        if m_foreach:
            var_name = m_foreach.group(1)
            vals = [v.strip().strip('"') for v in m_foreach.group(2).split(',') if v.strip()]
            block_lines = []
            depth = 1
            i += 1
            while i < len(lines_raw) and depth > 0:
                if '{' in lines_raw[i]: depth += lines_raw[i].count('{')
                if '}' in lines_raw[i]: depth -= lines_raw[i].count('}')
                if depth > 0:
                    block_lines.append(lines_raw[i])
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

    seen = set()
    unique_libcalls = []
    for lc in libcalls:
        if lc not in seen:
            seen.add(lc)
            unique_libcalls.append(lc)

    # RuntimeLibcallImpl
    impls = re.findall(r'def\s+([A-Za-z0-9_]+)\s*:\s*(?:[A-Za-z0-9_]+)?RuntimeLibcallImpl', content)
    unique_impls = []
    seen_impl = set()
    for imp in impls:
        if imp not in seen_impl:
            seen_impl.add(imp)
            unique_impls.append(imp)

    # Families
    families = []
    for m in re.finditer(r'def\s*:\s*RuntimeLibcallFamily\s*<\s*"([^"]+)"', content):
        base = m.group(1)
        if base not in families:
            families.append(base)

    families.sort()

    lines = []
    lines.append("/*===- TableGen'erated file ----------------------------------*- C++ -*-===*\\")
    lines.append("|*                                                                            *|")
    lines.append("|* Runtime LibCalls Source Fragment                                           *|")
    lines.append("|*                                                                            *|")
    lines.append("\\*===----------------------------------------------------------------------===*/")
    lines.append("")

    # GET_RUNTIME_LIBCALL_ENUM
    lines.append("#ifdef GET_RUNTIME_LIBCALL_ENUM")
    lines.append("#undef GET_RUNTIME_LIBCALL_ENUM")
    lines.append("namespace llvm {")
    lines.append("namespace RTLIB {")
    lines.append("enum Libcall : unsigned short {")
    for idx, lc in enumerate(unique_libcalls):
        lines.append(f"  {lc} = {idx},")
    lines.append(f"  UNKNOWN_LIBCALL = {len(unique_libcalls)}")
    lines.append("};")
    lines.append("")
    lines.append("enum LibcallImpl : unsigned short {")
    lines.append("  Unsupported = 0,")
    for idx, imp in enumerate(unique_impls, 1):
        lines.append(f"  impl_{imp} = {idx},")
    lines.append("};")
    lines.append(f"constexpr size_t NumLibcallImpls = {len(unique_impls) + 1};")
    lines.append("} // End namespace RTLIB")
    lines.append("} // End namespace llvm")
    lines.append("#endif")
    lines.append("")

    # GET_LOOKUP_LIBCALL_IMPL_NAME_BODY
    lines.append("#ifdef GET_LOOKUP_LIBCALL_IMPL_NAME_BODY")
    lines.append("#undef GET_LOOKUP_LIBCALL_IMPL_NAME_BODY")
    lines.append("  return enum_seq(RTLIB::Unsupported, RTLIB::Unsupported);")
    lines.append("#endif")
    lines.append("")

    # GET_RUNTIME_LIBCALLS_INFO_MEMBER_DECLS
    lines.append("#ifdef GET_RUNTIME_LIBCALLS_INFO_MEMBER_DECLS")
    lines.append("#undef GET_RUNTIME_LIBCALLS_INFO_MEMBER_DECLS")
    lines.append("#endif")
    lines.append("")

    # GET_RUNTIME_LIBCALL_FP_SELECTOR_DECLS
    lines.append("#ifdef GET_RUNTIME_LIBCALL_FP_SELECTOR_DECLS")
    lines.append("#undef GET_RUNTIME_LIBCALL_FP_SELECTOR_DECLS")
    for fam in families:
        lines.append(f"LLVM_ABI Libcall get{fam}(EVT VT);")
    lines.append("#endif")
    lines.append("")

    out_file = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\IR\RuntimeLibcalls.inc")
    out_file.write_text("\n".join(lines), encoding="utf-8")
    print(f"Generated {out_file} with {len(unique_libcalls)} libcalls, {len(unique_impls)} impls, and {len(families)} families ({len(lines)} lines)")

if __name__ == "__main__":
    generate_runtime_libcalls()
