import re
from pathlib import Path

def generate_intrinsic_enums():
    td_path = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\IR\Intrinsics.td")
    content = td_path.read_text(encoding="utf-8")
    content = re.sub(r"//.*", "", content)

    # Find all def int_...
    # Example: def int_vastart : ...
    pattern = re.compile(r"def\s+(int_[A-Za-z0-9_]+)\s*:\s*([A-Za-z0-9_]+)", re.MULTILINE)
    matches = pattern.findall(content)

    lines = []
    lines.append("/*===- TableGen'erated file ----------------------------------*- C++ -*-===*\\")
    lines.append("|*                                                                            *|")
    lines.append("|* Intrinsic Enums Source Fragment                                            *|")
    lines.append("|*                                                                            *|")
    lines.append("\\*===----------------------------------------------------------------------===*/")
    lines.append("")

    lines.append("#ifdef GET_INTRINSIC_ENUM_VALUES")
    lines.append("#undef GET_INTRINSIC_ENUM_VALUES")
    
    first = True
    count = 0
    for def_name, _ in matches:
        enum_name = def_name[4:] # strip "int_"
        if first:
            lines.append(f"    {enum_name} = 1,")
            first = False
        else:
            lines.append(f"    {enum_name},")
        count += 1

    lines.append(f"    num_intrinsics = {count + 1}")
    lines.append("#endif")
    lines.append("")

    lines.append("#ifdef GET_INTRINSIC_ANYKIND_ENUMS")
    lines.append("#undef GET_INTRINSIC_ANYKIND_ENUMS")
    lines.append("enum AnyKindVectorConstraint {")
    lines.append("  VC_None = 0,")
    lines.append("  VC_Vector = 1,")
    lines.append("  VC_Scalar = 2,")
    lines.append("};")
    lines.append("")
    lines.append("enum AnyKindElementConstraint {")
    lines.append("  EC_None = 0,")
    lines.append("  EC_Integer = 1,")
    lines.append("  EC_Float = 2,")
    lines.append("  EC_Pointer = 3,")
    lines.append("};")
    lines.append("#endif")
    lines.append("")

    out_file = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\IR\IntrinsicEnums.inc")
    out_file.write_text("\n".join(lines), encoding="utf-8")
    print(f"Generated {out_file} with {count} intrinsics ({len(lines)} lines)")

if __name__ == "__main__":
    generate_intrinsic_enums()
