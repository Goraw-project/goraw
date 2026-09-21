/* Рантайм JIT-специализации для Goraw.
 *
 * Идея: компилятор Goraw генерирует IR-ШАБЛОН функции, где захваченные
 * переменные оставлены плейсхолдерами `$CAP0$`, `$CAP1$`, ... . В рантайме,
 * в точке вычисления `jit(...)`, мы подставляем реальные (уже известные)
 * значения захватов как КОНСТАНТЫ, после чего LLVM ORC компилирует
 * специализированную функцию в машинный код и возвращает на неё указатель.
 * Это и есть «разворачивание на лету».
 *
 * LLVM-C грузится динамически (LoadLibrary/GetProcAddress) — ни заголовки,
 * ни импорт-библиотеки LLVM для сборки не нужны. Требуется лишь LLVM-C.dll
 * в PATH (каталог LLVM\bin).
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* --- минимальные объявления kernel32 (чтобы не тянуть windows.h) --- */
extern void *__stdcall LoadLibraryA(const char *name);
extern void *__stdcall GetProcAddress(void *module, const char *name);

/* --- непрозрачные типы LLVM-C --- */
typedef void *LLVMContextRef;
typedef void *LLVMModuleRef;
typedef void *LLVMMemoryBufferRef;
typedef void *LLVMErrorRef;
typedef void *LLVMOrcThreadSafeContextRef;
typedef void *LLVMOrcThreadSafeModuleRef;
typedef void *LLVMOrcLLJITRef;
typedef void *LLVMOrcLLJITBuilderRef;
typedef void *LLVMOrcJITDylibRef;
typedef void *LLVMOrcDefinitionGeneratorRef;
typedef int LLVMBool;

/* --- сигнатуры нужных функций LLVM-C --- */
typedef void (*fn_init_void)(void);
typedef LLVMContextRef (*fn_ctx_create)(void);
typedef LLVMMemoryBufferRef (*fn_membuf_copy)(const char *, size_t, const char *);
typedef LLVMBool (*fn_parse_ir)(LLVMContextRef, LLVMMemoryBufferRef, LLVMModuleRef *, char **);
typedef LLVMOrcThreadSafeContextRef (*fn_tsc_from_ctx)(LLVMContextRef);
typedef LLVMOrcThreadSafeModuleRef (*fn_tsm_create)(LLVMModuleRef, LLVMOrcThreadSafeContextRef);
typedef LLVMErrorRef (*fn_create_lljit)(LLVMOrcLLJITRef *, LLVMOrcLLJITBuilderRef);
typedef LLVMOrcJITDylibRef (*fn_main_jd)(LLVMOrcLLJITRef);
typedef LLVMErrorRef (*fn_add_module)(LLVMOrcLLJITRef, LLVMOrcJITDylibRef, LLVMOrcThreadSafeModuleRef);
typedef LLVMErrorRef (*fn_lookup)(LLVMOrcLLJITRef, uint64_t *, const char *);
typedef char (*fn_global_prefix)(LLVMOrcLLJITRef);
typedef LLVMErrorRef (*fn_dynlib_gen)(LLVMOrcDefinitionGeneratorRef *, char, void *, void *);
typedef void (*fn_add_generator)(LLVMOrcJITDylibRef, LLVMOrcDefinitionGeneratorRef);
typedef char *(*fn_error_msg)(LLVMErrorRef);

static struct {
    int loaded;
    fn_init_void init_info, init_target, init_mc, init_asmprinter, init_asmparser;
    fn_ctx_create ctx_create;
    fn_membuf_copy membuf_copy;
    fn_parse_ir parse_ir;
    fn_tsc_from_ctx tsc_from_ctx;
    fn_tsm_create tsm_create;
    fn_create_lljit create_lljit;
    fn_main_jd main_jd;
    fn_add_module add_module;
    fn_lookup lookup;
    fn_global_prefix global_prefix;
    fn_dynlib_gen dynlib_gen;
    fn_add_generator add_generator;
    fn_error_msg error_msg;
} L;

static LLVMOrcLLJITRef g_jit = 0; /* один общий LLJIT на процесс */

#define RESOLVE(field, name)                                        \
    do {                                                            \
        L.field = (void *)GetProcAddress(dll, name);                \
        if (!L.field) {                                             \
            fprintf(stderr, "goraw-jit: нет символа %s\n", name);   \
            return 0;                                               \
        }                                                           \
    } while (0)

static int jit_load(void) {
    if (L.loaded)
        return 1;
    void *dll = LoadLibraryA("LLVM-C.dll");
    if (!dll) {
        fprintf(stderr, "goraw-jit: не удалось загрузить LLVM-C.dll (нет в PATH?)\n");
        return 0;
    }
    RESOLVE(init_info, "LLVMInitializeX86TargetInfo");
    RESOLVE(init_target, "LLVMInitializeX86Target");
    RESOLVE(init_mc, "LLVMInitializeX86TargetMC");
    RESOLVE(init_asmprinter, "LLVMInitializeX86AsmPrinter");
    RESOLVE(init_asmparser, "LLVMInitializeX86AsmParser");
    RESOLVE(ctx_create, "LLVMContextCreate");
    RESOLVE(membuf_copy, "LLVMCreateMemoryBufferWithMemoryRangeCopy");
    RESOLVE(parse_ir, "LLVMParseIRInContext");
    RESOLVE(tsc_from_ctx, "LLVMOrcCreateNewThreadSafeContextFromLLVMContext");
    RESOLVE(tsm_create, "LLVMOrcCreateNewThreadSafeModule");
    RESOLVE(create_lljit, "LLVMOrcCreateLLJIT");
    RESOLVE(main_jd, "LLVMOrcLLJITGetMainJITDylib");
    RESOLVE(add_module, "LLVMOrcLLJITAddLLVMIRModule");
    RESOLVE(lookup, "LLVMOrcLLJITLookup");
    RESOLVE(global_prefix, "LLVMOrcLLJITGetGlobalPrefix");
    RESOLVE(dynlib_gen, "LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess");
    RESOLVE(add_generator, "LLVMOrcJITDylibAddGenerator");
    RESOLVE(error_msg, "LLVMGetErrorMessage");

    L.init_info();
    L.init_target();
    L.init_mc();
    L.init_asmprinter();
    L.init_asmparser();
    L.loaded = 1;
    return 1;
}

/* Форматирует одно значение захвата как текст LLVM-константы. */
static void format_capture(char *buf, size_t n, long long bits, int kind) {
    switch (kind) {
    case 0: case 1: case 2: case 3: /* i8/i16/i32/i64 — знаковые */
        snprintf(buf, n, "%lld", bits);
        break;
    case 4: case 5: case 6: case 7: /* u8/u16/u32/u64 — беззнаковые */
        snprintf(buf, n, "%llu", (unsigned long long)bits);
        break;
    case 8: { /* f32: младшие 32 бита — это float, печатаем hex double */
        uint32_t u = (uint32_t)(uint64_t)bits;
        float f;
        memcpy(&f, &u, 4);
        double d = (double)f;
        uint64_t db;
        memcpy(&db, &d, 8);
        snprintf(buf, n, "0x%016llX", (unsigned long long)db);
        break;
    }
    case 9: { /* f64: биты — это double */
        snprintf(buf, n, "0x%016llX", (unsigned long long)bits);
        break;
    }
    default:
        snprintf(buf, n, "0");
    }
}

/* Заменяет все вхождения `needle` на `repl` в строке src -> новая malloc-строка. */
static char *replace_all(const char *src, const char *needle, const char *repl) {
    size_t nlen = strlen(needle), rlen = strlen(repl);
    size_t cap = strlen(src) + 1, len = 0;
    char *out = (char *)malloc(cap);
    const char *p = src;
    while (*p) {
        if (nlen && strncmp(p, needle, nlen) == 0) {
            while (len + rlen + 1 > cap) { cap *= 2; out = (char *)realloc(out, cap); }
            memcpy(out + len, repl, rlen);
            len += rlen;
            p += nlen;
        } else {
            if (len + 2 > cap) { cap *= 2; out = (char *)realloc(out, cap); }
            out[len++] = *p++;
        }
    }
    out[len] = 0;
    return out;
}

/* --- простой кэш специализаций: ключ = финальный IR-текст --- */
struct cache_node {
    char *key;
    void *addr;
    struct cache_node *next;
};
static struct cache_node *g_cache = 0;

static void *cache_get(const char *key) {
    for (struct cache_node *n = g_cache; n; n = n->next)
        if (strcmp(n->key, key) == 0)
            return n->addr;
    return 0;
}
static void cache_put(const char *key, void *addr) {
    struct cache_node *n = (struct cache_node *)malloc(sizeof(*n));
    n->key = strdup(key);
    n->addr = addr;
    n->next = g_cache;
    g_cache = n;
}

/* Главная точка входа, вызываемая из кода Goraw.
 * Возвращает указатель на скомпилированную функцию (или 0 при ошибке). */
void *goraw_jit_compile(const char *template_ir, const char *fn_name, int n_caps,
                        const long long *cap_bits, const int *cap_kinds) {
    if (!jit_load())
        return 0;

    /* 1. Подставляем захваты. */
    char *ir = strdup(template_ir);
    for (int i = 0; i < n_caps; i++) {
        char needle[32];
        char val[32];
        snprintf(needle, sizeof(needle), "$CAP%d$", i);
        format_capture(val, sizeof(val), cap_bits[i], cap_kinds[i]);
        char *next = replace_all(ir, needle, val);
        free(ir);
        ir = next;
    }

    /* 2. Кэш: одинаковая специализация не пересобирается. */
    void *cached = cache_get(ir);
    if (cached) {
        free(ir);
        return cached;
    }

    /* 3. Уникализируем имя функции: один общий LLJIT не терпит двух
     *    определений с одинаковым символом (разные специализации). */
    static long long g_counter = 0;
    char uniq[160];
    snprintf(uniq, sizeof(uniq), "%s_%lld", fn_name, g_counter++);
    char at_old[144], at_new[176];
    snprintf(at_old, sizeof(at_old), "@%s", fn_name);
    snprintf(at_new, sizeof(at_new), "@%s", uniq);
    char *renamed = replace_all(ir, at_old, at_new);

    /* 4. Парсим IR в свежий контекст. */
    LLVMContextRef ctx = L.ctx_create();
    LLVMMemoryBufferRef mb = L.membuf_copy(renamed, strlen(renamed), "goraw-jit");
    LLVMModuleRef mod = 0;
    char *err = 0;
    if (L.parse_ir(ctx, mb, &mod, &err)) {
        fprintf(stderr, "goraw-jit: ошибка разбора IR: %s\n", err ? err : "(нет текста)");
        fprintf(stderr, "----- IR -----\n%s\n--------------\n", renamed);
        free(renamed);
        free(ir);
        return 0;
    }
    free(renamed);

    /* 4. Общий LLJIT на процесс + генератор символов процесса (printf и пр.). */
    if (!g_jit) {
        LLVMErrorRef e = L.create_lljit(&g_jit, 0);
        if (e) {
            fprintf(stderr, "goraw-jit: CreateLLJIT: %s\n", L.error_msg(e));
            free(ir);
            return 0;
        }
        LLVMOrcJITDylibRef jd = L.main_jd(g_jit);
        LLVMOrcDefinitionGeneratorRef gen = 0;
        char prefix = L.global_prefix(g_jit);
        LLVMErrorRef ge = L.dynlib_gen(&gen, prefix, 0, 0);
        if (ge) {
            fprintf(stderr, "goraw-jit: DynLibGenerator: %s\n", L.error_msg(ge));
        } else {
            L.add_generator(jd, gen);
        }
    }

    /* 5. Добавляем модуль и ищем функцию. */
    LLVMOrcThreadSafeContextRef tsc = L.tsc_from_ctx(ctx);
    LLVMOrcThreadSafeModuleRef tsm = L.tsm_create(mod, tsc);
    LLVMOrcJITDylibRef jd = L.main_jd(g_jit);
    LLVMErrorRef ae = L.add_module(g_jit, jd, tsm);
    if (ae) {
        fprintf(stderr, "goraw-jit: AddModule: %s\n", L.error_msg(ae));
        free(ir);
        return 0;
    }
    uint64_t addr = 0;
    LLVMErrorRef le = L.lookup(g_jit, &addr, uniq);
    if (le) {
        fprintf(stderr, "goraw-jit: Lookup(%s): %s\n", uniq, L.error_msg(le));
        free(ir);
        return 0;
    }

    cache_put(ir, (void *)(uintptr_t)addr);
    free(ir);
    return (void *)(uintptr_t)addr;
}
