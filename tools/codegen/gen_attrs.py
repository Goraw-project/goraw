import re
from pathlib import Path

def generate_attributes():
    td_path = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\IR\Attributes.td")
    content = td_path.read_text(encoding="utf-8")
    content = re.sub(r"//.*", "", content)

    # Parse defs of form:
    # def Name : Class<"string"[, Intersect, [Prop, ...]]>;
    pattern = re.compile(
        r'def\s+([A-Za-z0-9_]+)\s*:\s*([A-Za-z0-9_]+)<"([^"]+)"(?:\s*,\s*([A-Za-z0-9_]+))?(?:\s*,\s*\[([^\]]*)\])?\s*>\s*;',
        re.DOTALL
    )

    matches = pattern.findall(content)

    by_kind = {}
    for name, cls, attr_str, intersect, props in matches:
        prop_list = [p.strip() for p in props.split(",") if p.strip()]
        if intersect:
            prop_list.append(intersect)
        by_kind.setdefault(cls, []).append({
            "name": name,
            "attr_str": attr_str,
            "props": prop_list
        })

    # Parse CompatRule
    # def : CompatRule<"func">;
    # def : CompatRuleStrAttr<"func", "attr">;
    compat_rules = []
    for m in re.finditer(r'def\s*:\s*CompatRule(?:StrAttr)?<"([^"]+)"(?:\s*,\s*"([^"]+)")?>\s*;', content):
        func_name = m.group(1)
        attr_name = m.group(2) or ""
        compat_rules.append((func_name, attr_name))

    # Parse MergeRule
    # def : MergeRule<"func">;
    merge_rules = []
    for m in re.finditer(r'def\s*:\s*MergeRule<"([^"]+)">\s*;', content):
        func_name = m.group(1)
        merge_rules.append(func_name)

    lines = []
    lines.append("/*===- TableGen'erated file ----------------------------------*- C++ -*-===*\\")
    lines.append("|*                                                                            *|")
    lines.append("|* Attributes Source Fragment                                                 *|")
    lines.append("|*                                                                            *|")
    lines.append("\\*===----------------------------------------------------------------------===*/")
    lines.append("")

    # GET_ATTR_NAMES
    lines.append("#ifdef GET_ATTR_NAMES")
    lines.append("#undef GET_ATTR_NAMES")
    lines.append("")
    lines.append("#ifndef ATTRIBUTE_ALL")
    lines.append("#define ATTRIBUTE_ALL(FIRST, SECOND)")
    lines.append("#endif")
    lines.append("")

    def emit_macro(kind_names, macro_name):
        lines.append(f"#ifndef {macro_name}")
        lines.append(f"#define {macro_name}(FIRST, SECOND) ATTRIBUTE_ALL(FIRST, SECOND)")
        lines.append("#endif")
        lines.append("")
        for kn in kind_names:
            for item in by_kind.get(kn, []):
                lines.append(f'{macro_name}({item["name"]},"{item["attr_str"]}")')
        lines.append(f"#undef {macro_name}")
        lines.append("")

    emit_macro(["EnumAttr", "TypeAttr", "IntAttr", "ConstantRangeAttr", "ConstantRangeListAttr"], "ATTRIBUTE_ENUM")
    emit_macro(["StrBoolAttr"], "ATTRIBUTE_STRBOOL")
    emit_macro(["ComplexStrAttr"], "ATTRIBUTE_COMPLEXSTR")

    lines.append("#undef ATTRIBUTE_ALL")
    lines.append("#endif")
    lines.append("")

    # GET_ATTR_ENUM
    lines.append("#ifdef GET_ATTR_ENUM")
    lines.append("#undef GET_ATTR_ENUM")
    value = 1
    for kind_name in ["EnumAttr", "TypeAttr", "IntAttr", "ConstantRangeAttr", "ConstantRangeListAttr"]:
        lines.append(f"First{kind_name} = {value},")
        for item in by_kind.get(kind_name, []):
            lines.append(f"{item['name']} = {value},")
            value += 1
        lines.append(f"Last{kind_name} = {value - 1},")
    lines.append("#endif")
    lines.append("")

    # GET_ATTR_COMPAT_FUNC
    lines.append("#ifdef GET_ATTR_COMPAT_FUNC")
    lines.append("#undef GET_ATTR_COMPAT_FUNC")
    lines.append("static inline bool hasCompatibleFnAttrs(const Function &Caller,")
    lines.append("                                        const Function &Callee) {")
    lines.append("  bool Ret = true;")
    lines.append("")
    for func_name, attr_name in compat_rules:
        if attr_name:
            lines.append(f'  Ret &= {func_name}(Caller, Callee, "{attr_name}");')
        else:
            lines.append(f'  Ret &= {func_name}(Caller, Callee);')
    lines.append("")
    lines.append("  return Ret;")
    lines.append("}")
    lines.append("")
    lines.append("static inline void mergeFnAttrs(Function &Caller,")
    lines.append("                                const Function &Callee) {")
    for func_name in merge_rules:
        lines.append(f"  {func_name}(Caller, Callee);")
    lines.append("}")
    lines.append("")
    lines.append("#endif")
    lines.append("")

    # GET_ATTR_PROP_TABLE
    lines.append("#ifdef GET_ATTR_PROP_TABLE")
    lines.append("#undef GET_ATTR_PROP_TABLE")
    lines.append("static const uint8_t AttrPropTable[] = {")
    for kind_name in ["EnumAttr", "TypeAttr", "IntAttr", "ConstantRangeAttr", "ConstantRangeListAttr"]:
        for item in by_kind.get(kind_name, []):
            props_code = "".join(f" | AttributeProperty::{p}" for p in item["props"])
            lines.append(f"0{props_code},")
    lines.append("};")
    lines.append("#endif")
    lines.append("")

    out_file = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\IR\Attributes.inc")
    out_file.write_text("\n".join(lines), encoding="utf-8")
    print(f"Generated {out_file} ({len(lines)} lines)")

if __name__ == "__main__":
    generate_attributes()
