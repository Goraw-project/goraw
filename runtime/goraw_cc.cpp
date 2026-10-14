// Статически линкуемый шим Goraw: заменяет внешний clang/llvm.
//
// Экспортирует C-ABI функции, которыми gorawc пользуется вместо запуска
// clang.exe:
//   * goraw_llc        — текстовый LLVM IR -> объектный файл (.obj) через LLVM
//   * goraw_cc_emit_bc — C/C++ исходник   -> LLVM bitcode (.bc) через clang
//   * goraw_cc_ast_json— C/C++ исходник   -> AST в JSON (строка) через clang
//
// Все тяжёлые зависимости (LLVM X86 codegen, clang frontend/tooling) линкуются
// статически в gorawc, так что системный LLVM/Clang больше не нужен.

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

#include "llvm/IR/LLVMContext.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/LegacyPassManager.h"
#include "llvm/IRReader/IRReader.h"
#include "llvm/Bitcode/BitcodeWriter.h"
#include "llvm/MC/TargetRegistry.h"
#include "llvm/Support/CodeGen.h"
#include "llvm/Support/FileSystem.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/SourceMgr.h"
#include "llvm/Support/TargetSelect.h"
#include "llvm/Support/raw_ostream.h"
#include "llvm/Target/TargetMachine.h"
#include "llvm/Target/TargetOptions.h"
#include "llvm/TargetParser/Host.h"

#include "clang/AST/ASTConsumer.h"
#include "clang/AST/ASTDumperUtils.h"
#include "clang/CodeGen/CodeGenAction.h"
#include "clang/Frontend/ASTConsumers.h"
#include "clang/Frontend/CompilerInstance.h"
#include "clang/Frontend/FrontendAction.h"
#include "clang/Tooling/Tooling.h"

// ---------------------------------------------------------------------------
// Общее
// ---------------------------------------------------------------------------

extern "C" void goraw_cc_free(void *p) { std::free(p); }

static char *dup_cstr(const std::string &s) {
    char *out = static_cast<char *>(std::malloc(s.size() + 1));
    if (!out) return nullptr;
    std::memcpy(out, s.data(), s.size());
    out[s.size()] = '\0';
    return out;
}

static bool g_llvm_inited = false;
static void ensure_llvm_init() {
    if (g_llvm_inited) return;
    LLVMInitializeX86TargetInfo();
    LLVMInitializeX86Target();
    LLVMInitializeX86TargetMC();
    LLVMInitializeX86AsmPrinter();
    LLVMInitializeX86AsmParser();
    g_llvm_inited = true;
}

// ---------------------------------------------------------------------------
// llvm::Module -> объектный файл / ассемблер (общий помощник)
// ---------------------------------------------------------------------------
//
// emit_obj : 1 -> объектник (.obj), 0 -> ассемблер (.s)
// Триплет берётся из самого модуля, если задан; иначе хост.
// Возвращает пустую строку при успехе, иначе текст ошибки.
static std::string module_to_file(llvm::Module &mod, const std::string &out_path,
                                  bool emit_obj, int opt_level) {
    using namespace llvm;

    std::string tstr = mod.getTargetTriple().str();
    if (tstr.empty()) tstr = sys::getDefaultTargetTriple();
    Triple triple_obj(tstr);
    mod.setTargetTriple(triple_obj);

    std::string terr;
    const Target *target = TargetRegistry::lookupTarget(triple_obj, terr);
    if (!target) return "lookupTarget: " + terr;

    TargetOptions opt;
    CodeGenOptLevel ol = CodeGenOptLevel::Default;
    switch (opt_level) {
        case 0: ol = CodeGenOptLevel::None; break;
        case 1: ol = CodeGenOptLevel::Less; break;
        case 2: ol = CodeGenOptLevel::Default; break;
        default: ol = CodeGenOptLevel::Aggressive; break;
    }
    std::unique_ptr<TargetMachine> tm(target->createTargetMachine(
        triple_obj, "generic", "", opt, std::optional<Reloc::Model>(Reloc::PIC_),
        std::nullopt, ol));
    if (!tm) return "createTargetMachine failed";
    mod.setDataLayout(tm->createDataLayout());

    std::error_code ec;
    raw_fd_ostream dest(out_path, ec, sys::fs::OF_None);
    if (ec) return "open out: " + ec.message();

    legacy::PassManager pm;
    CodeGenFileType ft = emit_obj ? CodeGenFileType::ObjectFile : CodeGenFileType::AssemblyFile;
    if (tm->addPassesToEmitFile(pm, dest, nullptr, ft))
        return "addPassesToEmitFile: unsupported";
    pm.run(mod);
    dest.flush();
    return std::string();
}

// ---------------------------------------------------------------------------
// IR -> объектный файл
// ---------------------------------------------------------------------------
//
// ir_text   : текстовый LLVM IR (нуль-терминированный)
// out_path  : путь для записи результата
// emit_obj  : 1 -> объектный файл (.obj), 0 -> ассемблер (.s)
// opt_level : 0..3
// triple    : целевой triple или NULL/"" для хоста
// err_out   : если !=NULL, туда кладётся malloc-строка с ошибкой (освободить goraw_cc_free)
//
// Возвращает 0 при успехе.
extern "C" int goraw_llc(const char *ir_text, const char *out_path, int emit_obj,
                         int opt_level, const char *triple, char **err_out) {
    using namespace llvm;
    if (err_out) *err_out = nullptr;
    ensure_llvm_init();

    LLVMContext ctx;
    SMDiagnostic diag;
    std::unique_ptr<MemoryBuffer> buf =
        MemoryBuffer::getMemBuffer(StringRef(ir_text), "goraw-ir", /*RequiresNullTerminator=*/false);
    std::unique_ptr<Module> mod = parseIR(buf->getMemBufferRef(), diag, ctx);
    if (!mod) {
        if (err_out) {
            std::string msg;
            raw_string_ostream os(msg);
            diag.print("goraw-llc", os);
            os.flush();
            *err_out = dup_cstr(msg);
        }
        return 1;
    }

    if (triple && triple[0]) mod->setTargetTriple(Triple(std::string(triple)));

    std::string err = module_to_file(*mod, out_path, emit_obj != 0, opt_level);
    if (!err.empty()) {
        if (err_out) *err_out = dup_cstr(err);
        return 4;
    }
    return 0;
}

// ---------------------------------------------------------------------------
// Общая часть для clang-операций
// ---------------------------------------------------------------------------

static std::vector<std::string> collect_args(const char *const *argv, int argc) {
    std::vector<std::string> args;
    args.reserve(argc);
    for (int i = 0; i < argc; i++) args.emplace_back(argv[i]);
    return args;
}

// ---------------------------------------------------------------------------
// C/C++ -> объектник/биткод
// ---------------------------------------------------------------------------
//
// clang::tooling уничтожает экшен сразу после прогона, поэтому вывод из
// EmitObj/EmitBCAction не успевает попасть на диск. Вместо этого гоним
// EmitLLVMOnlyAction (кодоген в llvm::Module в памяти), а записываем результат
// сами в EndSourceFileAction — пока модуль жив и мы владеем путём вывода.
namespace {
enum class OutKind { Obj, Bc };

class EmitToFileAction : public clang::EmitLLVMOnlyAction {
public:
    std::string OutPath;
    OutKind Kind;
    int OptLevel;
    std::string *Err;
    bool *Wrote;
    EmitToFileAction(llvm::LLVMContext *ctx, std::string out, OutKind k, int opt,
                     std::string *err, bool *wrote)
        : clang::EmitLLVMOnlyAction(ctx), OutPath(std::move(out)), Kind(k),
          OptLevel(opt), Err(err), Wrote(wrote) {}

    void EndSourceFileAction() override {
        // База наполняет TheModule — сперва её, потом забираем модуль.
        clang::EmitLLVMOnlyAction::EndSourceFileAction();
        std::unique_ptr<llvm::Module> M = takeModule();
        if (!M) {
            if (Err) *Err = "codegen не дал модуль (ошибки парса?)";
            return;
        }
        if (Kind == OutKind::Bc) {
            std::error_code ec;
            llvm::raw_fd_ostream os(OutPath, ec, llvm::sys::fs::OF_None);
            if (ec) {
                if (Err) *Err = "open bc: " + ec.message();
                return;
            }
            llvm::WriteBitcodeToFile(*M, os);
            os.flush();
        } else {
            std::string e = module_to_file(*M, OutPath, /*emit_obj=*/true, OptLevel);
            if (!e.empty()) {
                if (Err) *Err = e;
                return;
            }
        }
        if (Wrote) *Wrote = true;
    }
};

// Вытаскивает `-o <path>` и `-O<n>` из driver-аргументов.
static void extract_out_and_opt(std::vector<std::string> &args,
                                std::string &out_path, int &opt_level) {
    opt_level = 2;
    for (size_t i = 0; i < args.size();) {
        if (args[i] == "-o" && i + 1 < args.size()) {
            out_path = args[i + 1];
            args.erase(args.begin() + i, args.begin() + i + 2);
        } else if (args[i].rfind("-O", 0) == 0 && args[i].size() == 3) {
            char c = args[i][2];
            if (c >= '0' && c <= '3') opt_level = c - '0';
            ++i;
        } else {
            ++i;
        }
    }
}

static int run_emit(const char *source, int is_cpp, const char *const *argv,
                    int argc, OutKind kind, char **err_out) {
    if (err_out) *err_out = nullptr;
    ensure_llvm_init();
    std::vector<std::string> args = collect_args(argv, argc);
    std::string out_path;
    int opt_level = 2;
    extract_out_and_opt(args, out_path, opt_level);
    if (out_path.empty()) {
        if (err_out) *err_out = dup_cstr("не задан -o <path>");
        return 1;
    }
    llvm::LLVMContext ctx;
    std::string err;
    bool wrote = false;
    const char *fname = is_cpp ? "goraw_insert.cpp" : "goraw_insert.c";
    bool ok = clang::tooling::runToolOnCodeWithArgs(
        std::make_unique<EmitToFileAction>(&ctx, out_path, kind, opt_level, &err,
                                           &wrote),
        llvm::StringRef(source), args, fname, "goraw-cc");
    if (!wrote) {
        if (err_out)
            *err_out = dup_cstr(err.empty()
                                    ? (ok ? "вывод не записан" : "clang: ошибки компиляции")
                                    : err);
        return 1;
    }
    return 0;
}
} // namespace

// C/C++ -> LLVM bitcode (.bc). Нужен `-o <path>` в argv.
extern "C" int goraw_cc_emit_bc(const char *source, int is_cpp,
                                const char *const *argv, int argc,
                                char **err_out) {
    return run_emit(source, is_cpp, argv, argc, OutKind::Bc, err_out);
}

// C/C++ -> COFF объектник (.obj). Линкуется встроенным PE-компоновщиком Goraw.
// Нужен `-o <path>` в argv.
extern "C" int goraw_cc_emit_obj(const char *source, int is_cpp,
                                 const char *const *argv, int argc,
                                 char **err_out) {
    return run_emit(source, is_cpp, argv, argc, OutKind::Obj, err_out);
}

// ---------------------------------------------------------------------------
// C/C++ -> AST JSON
// ---------------------------------------------------------------------------

namespace {
class JsonDumpAction : public clang::ASTFrontendAction {
public:
    std::string *Out;
    std::string Filter;
    JsonDumpAction(std::string *o, std::string f) : Out(o), Filter(std::move(f)) {}
    std::unique_ptr<clang::ASTConsumer>
    CreateASTConsumer(clang::CompilerInstance &, llvm::StringRef) override {
        auto os = std::make_unique<llvm::raw_string_ostream>(*Out);
        return clang::CreateASTDumper(std::move(os), Filter,
                                      /*DumpDecls=*/true, /*Deserialize=*/false,
                                      /*DumpLookups=*/false, /*DumpDeclTypes=*/false,
                                      clang::ADOF_JSON);
    }
};
} // namespace

// Возвращает malloc-строку с AST в JSON (освободить goraw_cc_free) или NULL.
// filter : строка-фильтр ast-dump-filter или NULL.
extern "C" char *goraw_cc_ast_json(const char *source, int is_cpp,
                                   const char *const *argv, int argc,
                                   const char *filter) {
    std::vector<std::string> args = collect_args(argv, argc);
    args.push_back("-fsyntax-only");
    std::string result;
    std::string flt = filter ? std::string(filter) : std::string();
    const char *fname = is_cpp ? "goraw_input.cpp" : "goraw_input.c";
    bool ok = clang::tooling::runToolOnCodeWithArgs(
        std::make_unique<JsonDumpAction>(&result, flt), llvm::StringRef(source),
        args, fname, "goraw-cc");
    if (!ok && result.empty()) return nullptr;
    return dup_cstr(result);
}
