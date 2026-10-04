import re
from pathlib import Path

def generate_tli():
    td_path = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\Analysis\TargetLibraryInfo.td")
    content = td_path.read_text(encoding="utf-8")
    content = re.sub(r"//.*", "", content)

    # def name : TargetLibCall<"string", ...>;
    pattern = re.compile(
        r'def\s+([A-Za-z0-9_]+)\s*:\s*TargetLibCall<"([^"]+)"',
        re.MULTILINE
    )
    matches = pattern.findall(content)

    print(f"Found {len(matches)} TargetLibCall definitions")

    lines = []
    lines.append("/*===- TableGen'erated file ----------------------------------*- C++ -*-===*\\")
    lines.append("|*                                                                            *|")
    lines.append("|* TargetLibraryInfo Source Fragment                                          *|")
    lines.append("|*                                                                            *|")
    lines.append("\\*===----------------------------------------------------------------------===*/")
    lines.append("")

    # GET_TARGET_LIBRARY_INFO_ENUM
    lines.append("#ifdef GET_TARGET_LIBRARY_INFO_ENUM")
    lines.append("#undef GET_TARGET_LIBRARY_INFO_ENUM")
    lines.append("enum LibFunc : unsigned {")
    lines.append("  NotLibFunc = 0,")
    for name, _ in matches:
        lines.append(f"  LibFunc_{name},")
    lines.append("  NumLibFuncs,")
    lines.append("  End_LibFunc = NumLibFuncs,")
    if matches:
        lines.append(f"  Begin_LibFunc = LibFunc_{matches[0][0]},")
    else:
        lines.append("  Begin_LibFunc = NotLibFunc,")
    lines.append("};")
    lines.append("#endif")
    lines.append("")

    # GET_TARGET_LIBRARY_INFO_IMPL_DECL
    lines.append("#ifdef GET_TARGET_LIBRARY_INFO_IMPL_DECL")
    lines.append("#undef GET_TARGET_LIBRARY_INFO_IMPL_DECL")
    num_el = len(matches) + 1
    lines.append("LLVM_ABI static const llvm::StringTable StandardNamesStrTable;")
    lines.append(f"LLVM_ABI static const llvm::StringTable::Offset StandardNamesOffsets[{num_el}];")
    lines.append(f"LLVM_ABI static const uint8_t StandardNamesSizeTable[{num_el}];")
    lines.append("#endif")
    lines.append("")

    out_file = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\Analysis\TargetLibraryInfo.inc")
    out_file.write_text("\n".join(lines), encoding="utf-8")
    print(f"Generated {out_file} ({len(lines)} lines)")

if __name__ == "__main__":
    generate_tli()
