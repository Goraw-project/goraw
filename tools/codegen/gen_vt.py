import re
from pathlib import Path

def parse_value_types():
    td_path = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\CodeGen\ValueTypes.td")
    content = td_path.read_text(encoding="utf-8")

    # Strip line comments
    content = re.sub(r"//.*", "", content)

    # Locate defset block
    defset_m = re.search(r"defset\s+list<ValueType>\s+ValueTypes\s*=\s*\{(.*)\}\s*;\s*//\s*end", content, re.DOTALL)
    if not defset_m:
        defset_m = re.search(r"defset\s+list<ValueType>\s+ValueTypes\s*=\s*\{(.*)\}", content, re.DOTALL)
    block = defset_m.group(1) if defset_m else content

    # Split into normal and non-normal parts
    parts = re.split(r"let\s+isNormalValueType\s*=\s*false\s*in\s*\{", block)
    normal_block = parts[0]
    non_normal_block = parts[1] if len(parts) > 1 else ""

    def parse_defs_from_str(s, is_normal):
        items = []
        for m in re.finditer(r"def\s+([A-Za-z0-9_]+)\s*:\s*([A-Za-z0-9_]+)(?:<([^>]+)>)?\s*;", s, re.DOTALL):
            def_name = m.group(1)
            cls_name = m.group(2)
            args_str = m.group(3) or ""
            args = [a.strip() for a in args_str.split(",") if a.strip()]
            items.append((def_name, cls_name, args, is_normal))
        return items

    raw_defs = parse_defs_from_str(normal_block, True) + parse_defs_from_str(non_normal_block, False)

    defs = []
    for def_name, cls_name, args, is_normal in raw_defs:
        llvm_name = def_name
        size = 0
        nelem = 1
        elt_type = None
        is_overloaded = False
        is_integer = False
        is_fp = False
        is_vector = False
        is_scalable = False
        nf = 0
        is_riscv_tuple = False
        is_cheri = False
        
        if cls_name == "ValueType":
            size = int(args[0]) if args else 0
            if len(args) > 1:
                llvm_name = args[1].strip('"')
        elif cls_name == "VTInt":
            size = int(args[0])
            is_integer = True
        elif cls_name == "VTFP":
            size = int(args[0])
            is_fp = True
        elif cls_name == "VTAny":
            is_overloaded = True
        elif cls_name == "VTCheriCapability":
            size = int(args[0])
            is_cheri = True
        elif cls_name == "VTVec":
            nelem = int(args[0])
            elt_name = args[1]
            elt = next(d for d in defs if d["def_name"] == elt_name)
            size = nelem * elt["size"]
            elt_type = elt["def_name"]
            is_integer = elt["is_integer"]
            is_fp = elt["is_fp"]
            is_vector = True
            if len(args) > 2:
                llvm_name = args[2].strip('"')
        elif cls_name == "VTScalableVec":
            nelem = int(args[0])
            elt_name = args[1]
            elt = next(d for d in defs if d["def_name"] == elt_name)
            size = nelem * elt["size"]
            elt_type = elt["def_name"]
            is_integer = elt["is_integer"]
            is_fp = elt["is_fp"]
            is_vector = True
            is_scalable = True
        elif cls_name == "VTVecTup":
            size = int(args[0])
            nf = int(args[1])
            elt_name = args[2]
            elt_type = elt_name
            is_riscv_tuple = True
        else:
            print("Unknown class:", cls_name)
            
        defs.append({
            "def_name": def_name,
            "llvm_name": llvm_name,
            "size": size,
            "nelem": nelem if is_vector else 0,
            "elt_type": elt_type or "INVALID_SIMPLE_VALUE_TYPE",
            "is_overloaded": is_overloaded,
            "is_integer": is_integer,
            "is_fp": is_fp,
            "is_vector": is_vector,
            "is_scalable": is_scalable,
            "nf": nf,
            "is_riscv_tuple": is_riscv_tuple,
            "is_normal": is_normal,
            "is_cheri": is_cheri,
        })
        
    print(f"Parsed {len(defs)} ValueTypes")

    # Range tracker
    class RangeTracker:
        def __init__(self):
            self.ranges = {} # key -> [first, last, closed]
            
        def update(self, key, name, valid):
            if valid:
                if key not in self.ranges:
                    self.ranges[key] = [name, name, False]
                else:
                    assert not self.ranges[key][2], f"Gap detected for {key} at {name}"
                    self.ranges[key][1] = name
            else:
                if key in self.ranges:
                    self.ranges[key][2] = True

    rt = RangeTracker()
    
    lines_out = []
    lines_out.append("/*===- TableGen'erated file ----------------------------------*- C++ -*-===*\\")
    lines_out.append("|*                                                                            *|")
    lines_out.append("|* ValueType Source Fragment                                                  *|")
    lines_out.append("|*                                                                            *|")
    lines_out.append("\\*===----------------------------------------------------------------------===*/")
    lines_out.append("")
    lines_out.append("#ifdef GET_VT_ATTR // (Ty, sz, Any, Int, FP, Vec, Sc, Tup, NF, NElem, EltTy)")
    
    for vt in defs:
        name = vt["llvm_name"]
        is_int = vt["is_integer"]
        is_fp = vt["is_fp"]
        is_vec = vt["is_vector"]
        is_sc = vt["is_scalable"]
        is_tup = vt["is_riscv_tuple"]
        is_cheri = vt["is_cheri"]
        nf = vt["nf"]
        is_normal = vt["is_normal"]
        nelem = vt["nelem"]
        elt_name = vt["elt_type"]
        
        rt.update("INTEGER_FIXEDLEN_VECTOR_VALUETYPE", name, is_int and is_vec and not is_sc)
        rt.update("INTEGER_SCALABLE_VECTOR_VALUETYPE", name, is_int and is_sc)
        rt.update("FP_FIXEDLEN_VECTOR_VALUETYPE", name, is_fp and is_vec and not is_sc)
        rt.update("FP_SCALABLE_VECTOR_VALUETYPE", name, is_fp and is_sc)
        rt.update("FIXEDLEN_VECTOR_VALUETYPE", name, is_vec and not is_sc)
        rt.update("SCALABLE_VECTOR_VALUETYPE", name, is_sc)
        rt.update("RISCV_VECTOR_TUPLE_VALUETYPE", name, is_tup)
        rt.update("VECTOR_VALUETYPE", name, is_vec)
        rt.update("INTEGER_VALUETYPE", name, is_int and not is_vec)
        rt.update("FP_VALUETYPE", name, is_fp and not is_vec)
        rt.update("VALUETYPE", name, is_normal)
        rt.update("CHERI_CAPABILITY_VALUETYPE", name, is_cheri)
        
        int_code = (3 if name[0] == 'i' else 1) if is_int else 0
        fp_code = (3 if name[0] == 'f' else 1) if is_fp else 0
        
        lines_out.append(f"  GET_VT_ATTR({name}, {vt['size']}, {1 if vt['is_overloaded'] else 0}, {int_code}, {fp_code}, {1 if is_vec else 0}, {1 if is_sc else 0}, {1 if is_tup else 0}, {nf}, {nelem}, {elt_name})")

    lines_out.append("#endif")
    lines_out.append("")
    lines_out.append("#ifdef GET_VT_RANGES")
    for key, (first, last, _) in rt.ranges.items():
        lines_out.append(f"  FIRST_{key} = {first},")
        lines_out.append(f"  LAST_{key} = {last},")
    lines_out.append("#endif")
    lines_out.append("")
    
    # EVT section
    lines_out.append("#ifdef GET_VT_EVT")
    for vt in defs:
        is_int = vt["is_integer"]
        is_vec = vt["is_vector"]
        is_fp = vt["is_fp"]
        is_tup = vt["is_riscv_tuple"]
        
        if not (is_int or is_vec or is_fp or is_tup):
            continue
            
        name = vt["llvm_name"]
        
        if is_tup:
            nf = vt["nf"]
            sz = vt["size"]
            t_str = f'TargetExtType::get(Context, "riscv.vector.tuple", ScalableVectorType::get(Type::getInt8Ty(Context), {sz // (nf * 8)}), {nf})'
        else:
            prefix = ""
            suffix = ""
            if is_vec:
                kind = "Scalable" if vt["is_scalable"] else "Fixed"
                prefix = f"{kind}VectorType::get("
                suffix = f", {vt['nelem']})"
                
            out_elt = next(d for d in defs if d["def_name"] == vt["elt_type"]) if is_vec else vt
            out_sz = out_elt["size"]
            out_name = out_elt["llvm_name"]
            
            if out_elt["is_fp"]:
                if out_sz == 16:
                    flt_ty = "BFloatTy" if out_name == "bf16" else "HalfTy"
                elif out_sz == 32:
                    flt_ty = "FloatTy"
                elif out_sz == 64:
                    flt_ty = "DoubleTy"
                elif out_sz == 80:
                    flt_ty = "X86_FP80Ty"
                elif out_sz == 128:
                    flt_ty = "PPC_FP128Ty" if out_name == "ppcf128" else "FP128Ty"
                else:
                    raise ValueError(f"Unknown float size {out_sz}")
                base_str = f"Type::get{flt_ty}(Context)"
            elif out_elt["is_integer"]:
                if (out_sz & (out_sz - 1) == 0 and 8 <= out_sz <= 128) or out_sz == 1:
                    base_str = f"Type::getInt{out_sz}Ty(Context)"
                else:
                    base_str = f"Type::getIntNTy(Context, {out_sz})"
            else:
                raise ValueError(f"Unknown type {out_name}")
                
            t_str = f"{prefix}{base_str}{suffix}"
            
        lines_out.append(f"  GET_VT_EVT({name}, {t_str})")
        
    lines_out.append("#endif")
    lines_out.append("")
    
    out_dir = Path(r"p:\Goraw\llvm-project\llvm\include\llvm\CodeGen")
    out_dir.mkdir(parents=True, exist_ok=True)
    out_file = out_dir / "GenVT.inc"
    out_file.write_text("\n".join(lines_out), encoding="utf-8")
    print(f"Generated {out_file} ({len(lines_out)} lines)")

if __name__ == "__main__":
    parse_value_types()
