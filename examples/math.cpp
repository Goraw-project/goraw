// ============================================================================
// Сгенерировано компилятором Goraw (C++23 Backend)
// ============================================================================

#include <cstdint>
#include <cstddef>
#include <cstdlib>
#include <cstdio>
#include <cstring>
#include <string_view>
#include <string>
#include <type_traits>
#include <cmath>

struct GorawStr {
const char* ptr{""};
int64_t len{0};
constexpr GorawStr() = default;
constexpr GorawStr(const char* s) : ptr(s ? s : ""), len(s ? (int64_t)std::string_view(s).size() : 0) {}
constexpr GorawStr(const char* p, int64_t l) : ptr(p ? p : ""), len(l) {}
constexpr int64_t size() const noexcept { return len; }
constexpr int64_t length() const noexcept { return len; }
constexpr bool empty() const noexcept { return len == 0; }
constexpr bool is_empty() const noexcept { return len == 0; }
constexpr std::string_view view() const noexcept { return {ptr, (size_t)len}; }
operator std::string_view() const noexcept { return view(); }
const char* c_str() const noexcept { return ptr; }
bool starts_with(std::string_view prefix) const noexcept { return view().starts_with(prefix); }
bool ends_with(std::string_view suffix) const noexcept { return view().ends_with(suffix); }
uint8_t operator[](int64_t idx) const noexcept { return static_cast<uint8_t>(ptr[idx]); }
GorawStr clone() const {
if (len <= 0) return GorawStr();
char* buf = static_cast<char*>(std::malloc(len + 1));
std::memcpy(buf, ptr, len);
buf[len] = '\0';
return GorawStr(buf, len);
}
friend bool operator==(const GorawStr& a, const GorawStr& b) noexcept { return a.view() == b.view(); }
friend bool operator!=(const GorawStr& a, const GorawStr& b) noexcept { return a.view() != b.view(); }
friend bool operator<(const GorawStr& a, const GorawStr& b) noexcept { return a.view() < b.view(); }
    friend GorawStr operator+(const GorawStr& a, const GorawStr& b) {
int64_t total = a.len + b.len;
char* buf = static_cast<char*>(std::malloc(total + 1));
if (a.len > 0) std::memcpy(buf, a.ptr, a.len);
if (b.len > 0) std::memcpy(buf + a.len, b.ptr, b.len);
buf[total] = '\0';
return GorawStr(buf, total);
}
};

inline GorawStr str_from_cstr(const char* s) noexcept { return GorawStr(s); }
inline GorawStr str_from_cstr(const uint8_t* s) noexcept { return GorawStr(reinterpret_cast<const char*>(s)); }

template <typename A, typename B>
constexpr auto gw_add(A&& a, B&& b) {
if constexpr (std::is_convertible_v<A, std::string_view> && std::is_convertible_v<B, std::string_view>) {
return GorawStr(a) + GorawStr(b);
} else {
return std::forward<A>(a) + std::forward<B>(b);
}
}

template <typename A, typename B>
constexpr bool gw_eq(const A& a, const B& b) {
if constexpr (std::is_convertible_v<A, std::string_view> && std::is_convertible_v<B, std::string_view>) {
return std::string_view(a) == std::string_view(b);
} else {
return a == b;
}
}

template <typename A, typename B>
constexpr bool gw_ne(const A& a, const B& b) {
if constexpr (std::is_convertible_v<A, std::string_view> && std::is_convertible_v<B, std::string_view>) {
return std::string_view(a) != std::string_view(b);
} else {
return a != b;
}
}

inline void panic(const char* msg = "panic") {
std::fprintf(stderr, "[GORAW PANIC] %s\n", msg);
std::abort();
}
inline void panic(GorawStr msg) {
std::fprintf(stderr, "[GORAW PANIC] %.*s\n", (int)msg.len, msg.ptr);
std::abort();
}
inline void goraw_panic(const char* msg) noexcept { panic(msg); }

using std::abs;
using std::ceil;
using std::clamp;
using std::max;
using std::pow;
using std::sqrt;

// --- Предварительные объявления функций ---
double hypot(double a, double b);

// --- Определения функций ---
double hypot(double a, double b) {
    return sqrt(((a * a) + (b * b)));
}

int main(int argc, char** argv) {
    auto h = hypot(3.0, 4.0);
    printf("hypot(3,4)      = %f\n", h);
    auto r = ceil(sqrt(abs((-15.9))));
    printf("ceil(sqrt|-15.9|)= %f\n", r);
    printf("pow(2, 10)      = %f\n", pow(2.0, 10.0));
    printf("clamp(42,0,10)  = %lld\n", clamp(42, 0, 10));
    printf("max(7, 3)       = %lld\n", max(7, 3));
    return 0;
}

