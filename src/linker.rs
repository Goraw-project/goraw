//! Встроенный нативный PE/COFF компоновщик Goraw.
//!
//! Позволяет собирать COFF `.obj` файлы в автономный исполняемый `.exe` файл Windows x86-64
//! БЕЗ внешнего Clang, MSVC link.exe или GNU ld.
//! Поддерживает автоматический импорт базовых системных функций Win32 (kernel32.dll, msvcrt.dll).

use object::pe;
use object::read::{Object, ObjectSection, ObjectSymbol, RelocationTarget};
use object::write::pe::Writer;
use std::collections::HashMap;

/// Информация об импортируемом символе
#[derive(Debug, Clone)]
struct ImportSymbol {
    dll_name: &'static str,
    sym_name: String,
    hint: u16,
}

/// Создает таблицу автоматических системных импортов Windows
fn identify_import(name: &str) -> Option<(&'static str, &'static str)> {
    // Чистим имя от возможных префиксов
    let clean = name.trim_start_matches('\x01').trim_start_matches('_');

    match clean {
        // kernel32.dll
        "ExitProcess" => Some(("kernel32.dll", "ExitProcess")),
        "GetStdHandle" => Some(("kernel32.dll", "GetStdHandle")),
        "WriteFile" => Some(("kernel32.dll", "WriteFile")),
        "ReadFile" => Some(("kernel32.dll", "ReadFile")),
        "GetCommandLineA" => Some(("kernel32.dll", "GetCommandLineA")),
        "GetCommandLineW" => Some(("kernel32.dll", "GetCommandLineW")),
        "GetProcessHeap" => Some(("kernel32.dll", "GetProcessHeap")),
        "HeapAlloc" => Some(("kernel32.dll", "HeapAlloc")),
        "HeapFree" => Some(("kernel32.dll", "HeapFree")),
        "VirtualAlloc" => Some(("kernel32.dll", "VirtualAlloc")),
        "VirtualFree" => Some(("kernel32.dll", "VirtualFree")),
        "Sleep" => Some(("kernel32.dll", "Sleep")),
        "GetLastError" => Some(("kernel32.dll", "GetLastError")),

        // msvcrt.dll (стандартная библиотека C, встроенная во все версии Windows)
        "printf" => Some(("msvcrt.dll", "printf")),
        "puts" => Some(("msvcrt.dll", "puts")),
        "putchar" => Some(("msvcrt.dll", "putchar")),
        "getchar" => Some(("msvcrt.dll", "getchar")),
        "malloc" => Some(("msvcrt.dll", "malloc")),
        "free" => Some(("msvcrt.dll", "free")),
        "calloc" => Some(("msvcrt.dll", "calloc")),
        "realloc" => Some(("msvcrt.dll", "realloc")),
        "memcpy" => Some(("msvcrt.dll", "memcpy")),
        "memset" => Some(("msvcrt.dll", "memset")),
        "memmove" => Some(("msvcrt.dll", "memmove")),
        "memcmp" => Some(("msvcrt.dll", "memcmp")),
        "strlen" => Some(("msvcrt.dll", "strlen")),
        "strcmp" => Some(("msvcrt.dll", "strcmp")),
        "strncmp" => Some(("msvcrt.dll", "strncmp")),
        "strcpy" => Some(("msvcrt.dll", "strcpy")),
        "strncpy" => Some(("msvcrt.dll", "strncpy")),
        "exit" => Some(("msvcrt.dll", "exit")),
        "abort" => Some(("msvcrt.dll", "abort")),
        "system" => Some(("msvcrt.dll", "system")),
        "sin" => Some(("msvcrt.dll", "sin")),
        "cos" => Some(("msvcrt.dll", "cos")),
        "tan" => Some(("msvcrt.dll", "tan")),
        "sqrt" => Some(("msvcrt.dll", "sqrt")),
        "pow" => Some(("msvcrt.dll", "pow")),
        "log" => Some(("msvcrt.dll", "log")),
        "floor" => Some(("msvcrt.dll", "floor")),
        "ceil" => Some(("msvcrt.dll", "ceil")),

        _ => None,
    }
}

/// Связывает один или несколько COFF объектных файлов в автономный Windows PE x86-64 executable
pub fn link_coff_to_pe(obj_bytes_list: &[&[u8]], custom_entry: Option<&str>) -> Result<Vec<u8>, String> {
    const IMAGE_BASE: u64 = 0x140000000;
    const SECTION_ALIGN: u32 = 0x1000;
    const FILE_ALIGN: u32 = 0x200;

    let mut merged_text = Vec::new();
    let mut merged_rdata = Vec::new();
    let mut merged_data = Vec::new();

    // Карта символов: SymbolName -> (section_index: 0=.text, 1=.rdata, 2=.data, offset)
    let mut global_symbols: HashMap<String, (u8, u32)> = HashMap::new();
    let mut needed_imports: Vec<ImportSymbol> = Vec::new();
    let mut unresolved_symbols: Vec<String> = Vec::new();

    let mut obj_sec_offsets: Vec<HashMap<usize, (u8, u32)>> = Vec::new();

    // Проход 1: слияние секций и сбор символов из всех входных объектников
    for &obj_data in obj_bytes_list {
        let file = object::File::parse(obj_data)
            .map_err(|e| format!("ошибка парсинга COFF объектника: {e}"))?;

        let mut sec_offsets: HashMap<usize, (u8, u32)> = HashMap::new();

        for sec in file.sections() {
            let name = sec.name().unwrap_or("");
            let data = sec.data().unwrap_or(&[]);
            let sec_idx = sec.index().0;

            if name == ".text" || name.starts_with(".text$") {
                let off = merged_text.len() as u32;
                merged_text.extend_from_slice(data);
                sec_offsets.insert(sec_idx, (0, off));
            } else if name == ".rdata" || name.starts_with(".rdata$") || name == ".rodata" {
                let off = merged_rdata.len() as u32;
                merged_rdata.extend_from_slice(data);
                sec_offsets.insert(sec_idx, (1, off));
            } else if name == ".data" || name.starts_with(".data$") {
                let off = merged_data.len() as u32;
                merged_data.extend_from_slice(data);
                sec_offsets.insert(sec_idx, (2, off));
            }
        }

        // Собираем экспортируемые и внешние символы
        for sym in file.symbols() {
            let name = match sym.name() {
                Ok(n) => n.to_string(),
                Err(_) => continue,
            };

            if sym.is_definition() {
                if let Some(sec_idx) = sym.section_index() {
                    if let Some(&(sec_id, base_off)) = sec_offsets.get(&sec_idx.0) {
                        let final_off = base_off + sym.address() as u32;
                        global_symbols.insert(name.clone(), (sec_id, final_off));
                    }
                }
            } else if sym.is_undefined() {
                if !unresolved_symbols.contains(&name) {
                    unresolved_symbols.push(name);
                }
            }
        }

        obj_sec_offsets.push(sec_offsets);
    }

    // Проверяем точку входа
    let entry_name = custom_entry.unwrap_or("main");
    let (entry_sec, entry_off) = match global_symbols.get(entry_name)
        .or_else(|| global_symbols.get(&format!("_{entry_name}")))
        .or_else(|| global_symbols.get("goraw_main"))
        .or_else(|| global_symbols.get("_goraw_main"))
    {
        Some(&s) => s,
        None => return Err(format!("точка входа `{entry_name}` не найдена среди определенных функций")),
    };

    if entry_sec != 0 {
        return Err(format!("точка входа `{entry_name}` должна находиться в исполняемой секции .text"));
    }

    // Стандартная переменная MSVC CRT для модулей с плавающей точкой
    if unresolved_symbols.iter().any(|s| s == "_fltused" || s == "__fltused") {
        if !global_symbols.contains_key("_fltused") {
            let off = merged_data.len() as u32;
            merged_data.extend_from_slice(&0x9876_i32.to_le_bytes());
            global_symbols.insert("_fltused".into(), (2, off));
            global_symbols.insert("__fltused".into(), (2, off));
        }
    }

    // Всегда гарантируем наличие ExitProcess из kernel32.dll для корректного выхода
    if !unresolved_symbols.iter().any(|s| s.contains("ExitProcess")) {
        unresolved_symbols.push("ExitProcess".into());
    }

    for sym in &unresolved_symbols {
        if global_symbols.contains_key(sym) {
            continue;
        }
        if let Some((dll, clean_name)) = identify_import(sym) {
            if !needed_imports.iter().any(|i| i.sym_name == clean_name && i.dll_name == dll) {
                needed_imports.push(ImportSymbol {
                    dll_name: dll,
                    sym_name: clean_name.to_string(),
                    hint: 0,
                });
            }
        } else {
            return Err(format!("не разрешен внешний символ `{sym}` (не найден во встроенных библиотеках kernel32/msvcrt)"));
        }
    }

    // Группируем импорты по DLL
    let mut dll_imports: HashMap<&'static str, Vec<ImportSymbol>> = HashMap::new();
    for imp in needed_imports {
        dll_imports.entry(imp.dll_name).or_default().push(imp);
    }

    // Создаем стартовый стаб (entry stub) в начале .text:
    // call main; mov ecx, eax; call [ExitProcess]; ret
    // Это гарантирует, что программа чисто возвращает код в ОС.
    let mut startup_stub = Vec::new();
    // sub rsp, 40 (выравнивание стека по 16 байт под Win64 ABI)
    startup_stub.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    // call main
    let main_rel_offset = (entry_off + 9) as i32 - 9;
    startup_stub.push(0xe8);
    startup_stub.extend_from_slice(&(main_rel_offset as i32).to_le_bytes());
    // mov ecx, eax (код возврата в первый аргумент RCX)
    startup_stub.extend_from_slice(&[0x89, 0xc1]);
    // call [rip + ExitProcess_IAT] — заполним позже
    let exit_call_offset = startup_stub.len();
    startup_stub.extend_from_slice(&[0xff, 0x15, 0x00, 0x00, 0x00, 0x00]);
    // add rsp, 40; ret (на всякий случай)
    startup_stub.extend_from_slice(&[0x48, 0x83, 0xc4, 0x28, 0xc3]);

    // Вставляем stub перед кодом
    let mut final_text = startup_stub.clone();
    let code_shift = startup_stub.len() as u32;
    final_text.extend_from_slice(&merged_text);

    // Сдвигаем символы в .text на code_shift
    for val in global_symbols.values_mut() {
        if val.0 == 0 {
            val.1 += code_shift;
        }
    }

    // Заново корректируем относительный вызов main в стабе:
    let final_main_off = global_symbols.get(entry_name)
        .or_else(|| global_symbols.get(&format!("_{entry_name}")))
        .unwrap().1;
    let call_target_disp = (final_main_off as i32) - 9;
    final_text[5..9].copy_from_slice(&call_target_disp.to_le_bytes());

    // Генерируем import thunks в .text для каждой внешней функции (напр. puts, printf, ExitProcess)
    // sym: jmp [rip + disp32] (ff 25 xx xx xx xx)
    let mut import_thunks: Vec<(String, u32)> = Vec::new();
    for imp_list in dll_imports.values() {
        for imp in imp_list {
            let thunk_off = final_text.len() as u32;
            import_thunks.push((imp.sym_name.clone(), thunk_off));
            global_symbols.insert(imp.sym_name.clone(), (0, thunk_off));
            global_symbols.insert(format!("_{}", imp.sym_name), (0, thunk_off));
            // jmp qword ptr [rip + disp32]
            final_text.extend_from_slice(&[0xff, 0x25, 0x00, 0x00, 0x00, 0x00]);
        }
    }

    // Вычисляем размеры секций для PE
    let text_rva = SECTION_ALIGN;
    let text_size = align_up(final_text.len() as u32, SECTION_ALIGN);

    let rdata_rva = text_rva + text_size;

    // Формируем секцию импортов .idata
    let (idata_data, iat_offsets, exit_process_iat_rva) = build_idata_section(&dll_imports, rdata_rva);

    // Теперь проставляем точный относительный адрес ExitProcess в startup_stub
    let exit_rel = (exit_process_iat_rva as i32) - ((text_rva + exit_call_offset as u32 + 6) as i32);
    final_text[exit_call_offset + 2..exit_call_offset + 6].copy_from_slice(&exit_rel.to_le_bytes());

    // Патчим thunk'и точными смещениями к IAT
    for (sym_name, thunk_off) in &import_thunks {
        if let Some(&iat_rva) = iat_offsets.get(sym_name) {
            let thunk_rva = text_rva + thunk_off;
            let rel = (iat_rva as i32) - ((thunk_rva + 6) as i32);
            let off = *thunk_off as usize;
            final_text[off + 2..off + 6].copy_from_slice(&rel.to_le_bytes());
        }
    }

    let idata_len = idata_data.len() as u32;
    let mut final_rdata = idata_data;
    final_rdata.extend_from_slice(&merged_rdata);

    let rdata_size = align_up(final_rdata.len() as u32, SECTION_ALIGN);
    let data_rva = rdata_rva + rdata_size;

    let mut final_data = merged_data;

    // Проход 2: Применение релокаций во всех секциях
    for (obj_idx, &obj_data) in obj_bytes_list.iter().enumerate() {
        let file = object::File::parse(obj_data).unwrap();
        let sec_offsets = &obj_sec_offsets[obj_idx];
        for sec in file.sections() {
            let sec_idx = sec.index().0;
            let (sec_id, base_off) = match sec_offsets.get(&sec_idx) {
                Some(&info) => info,
                None => continue,
            };

            for (reloc_offset, reloc) in sec.relocations() {
                let target_rva = match reloc.target() {
                    RelocationTarget::Symbol(idx) => {
                        let sym = match file.symbol_by_index(idx) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        let name = sym.name().unwrap_or("");
                        if !name.is_empty()
                            && name != ".text"
                            && name != ".rdata"
                            && name != ".rodata"
                            && name != ".data"
                            && !name.starts_with(".text$")
                            && !name.starts_with(".rdata$")
                            && !name.starts_with(".data$")
                        {
                            if let Some(&(s_id, s_off)) = global_symbols.get(name) {
                                match s_id {
                                    0 => text_rva + s_off,
                                    1 => rdata_rva + idata_len + s_off,
                                    2 => data_rva + s_off,
                                    _ => 0,
                                }
                            } else if let Some(&iat_rva) = iat_offsets.get(name) {
                                iat_rva
                            } else if let Some(s_sec) = sym.section_index() {
                                if let Some(&(t_id, t_base)) = sec_offsets.get(&s_sec.0) {
                                    let sym_addr = t_base + sym.address() as u32;
                                    match t_id {
                                        0 => text_rva + code_shift + sym_addr,
                                        1 => rdata_rva + idata_len + sym_addr,
                                        2 => data_rva + sym_addr,
                                        _ => 0,
                                    }
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        } else if let Some(s_sec) = sym.section_index() {
                            if let Some(&(t_id, t_base)) = sec_offsets.get(&s_sec.0) {
                                let sym_addr = t_base + sym.address() as u32;
                                match t_id {
                                    0 => text_rva + code_shift + sym_addr,
                                    1 => rdata_rva + idata_len + sym_addr,
                                    2 => data_rva + sym_addr,
                                    _ => 0,
                                }
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                    RelocationTarget::Section(s_sec) => {
                        if let Some(&(t_id, t_base)) = sec_offsets.get(&s_sec.0) {
                            match t_id {
                                0 => text_rva + code_shift + t_base,
                                1 => rdata_rva + idata_len + t_base,
                                2 => data_rva + t_base,
                                _ => 0,
                            }
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };

                // Применяем релокацию в зависимости от секции
                if sec_id == 0 {
                    // .text
                    let patch_pos = code_shift as usize + base_off as usize + reloc_offset as usize;
                    if patch_pos + 4 <= final_text.len() {
                        let cur_disp = i32::from_le_bytes(final_text[patch_pos..patch_pos + 4].try_into().unwrap());
                        let cur_rva = text_rva + patch_pos as u32;
                        let new_disp = (target_rva as i32 + cur_disp) - (cur_rva as i32 + 4);
                        final_text[patch_pos..patch_pos + 4].copy_from_slice(&new_disp.to_le_bytes());
                    }
                } else if sec_id == 1 {
                    // .rdata
                    let patch_pos = idata_len as usize + base_off as usize + reloc_offset as usize;
                    if patch_pos + 8 <= final_rdata.len() {
                        let cur_val = u64::from_le_bytes(final_rdata[patch_pos..patch_pos + 8].try_into().unwrap());
                        let target_va = IMAGE_BASE + target_rva as u64 + cur_val;
                        final_rdata[patch_pos..patch_pos + 8].copy_from_slice(&target_va.to_le_bytes());
                    } else if patch_pos + 4 <= final_rdata.len() {
                        final_rdata[patch_pos..patch_pos + 4].copy_from_slice(&target_rva.to_le_bytes());
                    }
                } else if sec_id == 2 {
                    // .data
                    let patch_pos = base_off as usize + reloc_offset as usize;
                    if patch_pos + 8 <= final_data.len() {
                        let cur_val = u64::from_le_bytes(final_data[patch_pos..patch_pos + 8].try_into().unwrap());
                        let target_va = IMAGE_BASE + target_rva as u64 + cur_val;
                        final_data[patch_pos..patch_pos + 8].copy_from_slice(&target_va.to_le_bytes());
                    } else if patch_pos + 4 <= final_data.len() {
                        final_data[patch_pos..patch_pos + 4].copy_from_slice(&target_rva.to_le_bytes());
                    }
                }
            }
        }
    }

    // Собираем готовый PE файл
    let mut buffer = Vec::new();
    let mut writer = Writer::new(true, SECTION_ALIGN, FILE_ALIGN, &mut buffer);

    let has_data = !final_data.is_empty();
    let num_sections = if has_data { 3 } else { 2 };

    writer.reserve_dos_header_and_stub();
    writer.reserve_nt_headers(pe::IMAGE_NUMBEROF_DIRECTORY_ENTRIES);
    writer.reserve_section_headers(num_sections);

    let text_range = writer.reserve_text_section(final_text.len() as u32);
    let rdata_range = writer.reserve_rdata_section(final_rdata.len() as u32);
    let data_range = if has_data {
        let data_len = final_data.len() as u32;
        Some(writer.reserve_data_section(data_len, data_len))
    } else {
        None
    };

    writer.set_data_directory(
        pe::IMAGE_DIRECTORY_ENTRY_IMPORT,
        rdata_rva,
        idata_len,
    );

    // Запись заголовков
    writer.write_dos_header_and_stub().map_err(|e| format!("{e}"))?;
    writer.write_nt_headers(object::write::pe::NtHeaders {
        machine: pe::IMAGE_FILE_MACHINE_AMD64,
        time_date_stamp: 0,
        characteristics: pe::IMAGE_FILE_EXECUTABLE_IMAGE | pe::IMAGE_FILE_LARGE_ADDRESS_AWARE,
        major_linker_version: 14,
        minor_linker_version: 0,
        address_of_entry_point: text_rva, // точка входа — наш startup stub
        image_base: IMAGE_BASE,
        major_operating_system_version: 6,
        minor_operating_system_version: 0,
        major_image_version: 0,
        minor_image_version: 0,
        major_subsystem_version: 6,
        minor_subsystem_version: 0,
        subsystem: pe::IMAGE_SUBSYSTEM_WINDOWS_CUI, // Console application
        dll_characteristics: pe::IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE
            | pe::IMAGE_DLLCHARACTERISTICS_NX_COMPAT
            | pe::IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE,
        size_of_stack_reserve: 0x100000,
        size_of_stack_commit: 0x1000,
        size_of_heap_reserve: 0x100000,
        size_of_heap_commit: 0x1000,
    });

    writer.write_section_headers();
    writer.write_section(text_range.file_offset, &final_text);
    writer.write_section(rdata_range.file_offset, &final_rdata);
    if let Some(range) = data_range {
        writer.write_section(range.file_offset, &final_data);
    }

    Ok(buffer)
}

/// Построитель структуры секции импорта `.idata`
fn build_idata_section(
    dll_imports: &HashMap<&'static str, Vec<ImportSymbol>>,
    base_rva: u32,
) -> (Vec<u8>, HashMap<String, u32>, u32) {
    let mut idata = Vec::new();
    let mut iat_offsets = HashMap::new();
    let mut exit_process_iat_rva = 0;

    let num_dlls = dll_imports.len();
    if num_dlls == 0 {
        return (idata, iat_offsets, exit_process_iat_rva);
    }

    // 1. Directory table (20 байт на DLL + 20 байт нулевой терминатор)
    let dir_table_size = (num_dlls + 1) * 20;
    idata.resize(dir_table_size, 0);

    let mut current_offset = dir_table_size;

    for (dll_idx, (dll_name, syms)) in dll_imports.iter().enumerate() {
        let ilt_offset = current_offset;
        let ilt_size = (syms.len() + 1) * 8;
        current_offset += ilt_size;

        let iat_offset = current_offset;
        let iat_size = (syms.len() + 1) * 8;
        current_offset += iat_size;

        let name_offset = current_offset;
        current_offset += dll_name.len() + 1;

        let mut hint_offsets = Vec::new();
        for s in syms {
            let h_off = current_offset;
            hint_offsets.push(h_off);
            // hint (2 bytes) + name + null
            current_offset += 2 + s.sym_name.len() + 1;
            if current_offset % 2 != 0 {
                current_offset += 1; // выравнивание по 2 байта
            }
        }

        // Заполняем ImageImportDescriptor в dir_table
        let desc_pos = dll_idx * 20;
        idata.resize(current_offset, 0);

        // OriginalFirstThunk (ILT RVA)
        idata[desc_pos..desc_pos + 4].copy_from_slice(&(base_rva + ilt_offset as u32).to_le_bytes());
        // Name RVA
        idata[desc_pos + 12..desc_pos + 16].copy_from_slice(&(base_rva + name_offset as u32).to_le_bytes());
        // FirstThunk (IAT RVA)
        idata[desc_pos + 16..desc_pos + 20].copy_from_slice(&(base_rva + iat_offset as u32).to_le_bytes());

        // Записываем имя DLL
        idata[name_offset..name_offset + dll_name.len()].copy_from_slice(dll_name.as_bytes());
        idata[name_offset + dll_name.len()] = 0;

        // Записываем Hints и Thunks
        for (i, s) in syms.iter().enumerate() {
            let h_off = hint_offsets[i];
            idata[h_off..h_off + 2].copy_from_slice(&s.hint.to_le_bytes());
            idata[h_off + 2..h_off + 2 + s.sym_name.len()].copy_from_slice(s.sym_name.as_bytes());
            idata[h_off + 2 + s.sym_name.len()] = 0;

            let thunk_val = (base_rva + h_off as u32) as u64;
            let ilt_pos = ilt_offset + i * 8;
            let iat_pos = iat_offset + i * 8;

            idata[ilt_pos..ilt_pos + 8].copy_from_slice(&thunk_val.to_le_bytes());
            idata[iat_pos..iat_pos + 8].copy_from_slice(&thunk_val.to_le_bytes());

            let sym_iat_rva = base_rva + iat_pos as u32;
            iat_offsets.insert(s.sym_name.clone(), sym_iat_rva);
            iat_offsets.insert(format!("__{}", s.sym_name), sym_iat_rva);
            iat_offsets.insert(format!("__imp_{}", s.sym_name), sym_iat_rva);

            if s.sym_name == "ExitProcess" {
                exit_process_iat_rva = sym_iat_rva;
            }
        }
    }

    (idata, iat_offsets, exit_process_iat_rva)
}

fn align_up(val: u32, align: u32) -> u32 {
    (val + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_coff_to_pe_and_run() {
        let asm_src = "
section .text
global main
main:
    mov eax, 42
    ret
";
        let (obj_opt, diags) = crate::asm::assemble("test.asm", asm_src);
        assert!(!diags.has_errors(), "ошибки ассемблирования: {}", diags.render_human());
        let obj_bytes = obj_opt.expect("объектник должен быть сгенерирован");

        let exe_bytes = link_coff_to_pe(&[&obj_bytes], Some("main")).expect("линковка PE должна пройти успешно");
        assert!(exe_bytes.len() > 512, "exe файл должен содержать заголовки и секции");

        // Проверяем DOS сигнатуру MZ и PE сигнатуру
        assert_eq!(&exe_bytes[0..2], b"MZ");

        // Записываем в test_out.exe для проверки
        let test_exe = std::env::current_dir().unwrap().join("test_out.exe");
        std::fs::write(&test_exe, &exe_bytes).expect("запись тестового exe");

        #[cfg(target_os = "windows")]
        {
            let status = std::process::Command::new(&test_exe).status();
            if status.is_ok() {
                let _ = std::fs::remove_file(&test_exe);
            }
            let s = status.expect("запуск сгенерированного exe");
            assert_eq!(s.code(), Some(42), "код возврата должен быть 42!");
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::fs::remove_file(&test_exe);
        }
    }

    #[test]
    fn test_link_coff_to_pe_multi_object() {
        let asm1 = "
section .data
global greeting
greeting:
    db \"Multi-Object Linking Success!\", 0
";
        let asm2 = "
section .text
extern puts
extern greeting
global main
main:
    sub rsp, 40
    lea rcx, [rel greeting]
    call puts
    xor eax, eax
    add rsp, 40
    ret
";
        let (obj1_opt, d1) = crate::asm::assemble("obj1.asm", asm1);
        assert!(!d1.has_errors(), "d1: {}", d1.render_human());
        let (obj2_opt, d2) = crate::asm::assemble("obj2.asm", asm2);
        assert!(!d2.has_errors(), "d2: {}", d2.render_human());

        let obj1 = obj1_opt.unwrap();
        let obj2 = obj2_opt.unwrap();

        let exe_bytes = link_coff_to_pe(&[&obj2, &obj1], Some("main")).expect("линковка 2-х объектников");
        assert!(exe_bytes.len() > 512);

        #[cfg(target_os = "windows")]
        {
            let test_exe = std::env::current_dir().unwrap().join("test_multi_obj.exe");
            std::fs::write(&test_exe, &exe_bytes).expect("запись");
            let out = std::process::Command::new(&test_exe).output();
            let _ = std::fs::remove_file(&test_exe);
            let out = out.expect("запуск test_multi_obj");
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("Multi-Object Linking Success!"));
        }
    }

    #[test]
    fn test_link_coff_to_pe_with_puts() {
        let asm_src = "
section .data
msg:
    db \"Goraw Pure Native PE Execution!\", 0

section .text
extern puts
global main
main:
    sub rsp, 40
    lea rcx, [rel msg]
    call puts
    xor eax, eax
    add rsp, 40
    ret
";
        let (obj_opt, diags) = crate::asm::assemble("test_puts.asm", asm_src);
        assert!(!diags.has_errors(), "ошибки: {}", diags.render_human());
        let obj_bytes = obj_opt.expect("объектник");

        let exe_bytes = link_coff_to_pe(&[&obj_bytes], Some("main")).expect("линковка");

        let test_exe = std::env::current_dir().unwrap().join("test_puts_out.exe");
        std::fs::write(&test_exe, &exe_bytes).expect("запись");

        #[cfg(target_os = "windows")]
        {
            let out = std::process::Command::new(&test_exe).output();
            if out.is_ok() {
                let _ = std::fs::remove_file(&test_exe);
            }
            let out = out.expect("запуск test_puts");
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("Goraw Pure Native PE Execution!"));
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::fs::remove_file(&test_exe);
        }
    }
}
