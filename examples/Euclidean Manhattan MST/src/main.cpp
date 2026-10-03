// ============================================================================
// Сгенерировано компилятором Goraw
// ============================================================================

#include <cstdint>
#include <cstddef>
#include <cstdlib>
#include <cstdio>
#include <cstring>
#include <cassert>
#include <cmath>
#include <string>
#include <string_view>
#include <vector>
#include <array>
#include <span>
#include <utility>
#include <algorithm>
#include <iostream>
#include <type_traits>

// Goraw базовые псевдонимы типов
using i8  = int8_t;
using i16 = int16_t;
using i32 = int32_t;
using i64 = int64_t;
using u8  = uint8_t;
using u16 = uint16_t;
using u32 = uint32_t;
using u64 = uint64_t;
using f32 = float;
using f64 = double;

// --- Goraw Runtime & Helper Types ---

struct GorawStr {
const char* ptr{nullptr};
int64_t len{0};
constexpr GorawStr() = default;
constexpr GorawStr(const char* s) : ptr(s), len(s ? (int64_t)std::string_view(s).size() : 0) {}
constexpr GorawStr(const char* p, int64_t l) : ptr(p), len(l) {}
constexpr int64_t size() const noexcept { return len; }
constexpr int64_t length() const noexcept { return len; }
constexpr bool empty() const noexcept { return len == 0; }
constexpr std::string_view view() const noexcept { return {ptr, (size_t)len}; }
operator std::string_view() const noexcept { return view(); }
const char* c_str() const noexcept { return ptr; }
char operator[](int64_t idx) const noexcept { return ptr[idx]; }
bool operator==(const GorawStr& o) const noexcept { return view() == o.view(); }
bool operator==(const char* o) const noexcept { return view() == o; }
bool operator!=(const GorawStr& o) const noexcept { return view() != o.view(); }
};

template <typename T>
struct GorawSlice {
T* ptr{nullptr};
int64_t len{0};
constexpr GorawSlice() = default;
constexpr GorawSlice(T* p, int64_t l) : ptr(p), len(l) {}
T& operator[](int64_t idx) { return ptr[idx]; }
const T& operator[](int64_t idx) const { return ptr[idx]; }
T* begin() noexcept { return ptr; }
T* end() noexcept { return ptr + len; }
const T* begin() const noexcept { return ptr; }
const T* end() const noexcept { return ptr + len; }
int64_t size() const noexcept { return len; }
int64_t length() const noexcept { return len; }
};

template <typename T>
constexpr decltype(auto) gw_deref(T&& obj) noexcept {
if constexpr (std::is_pointer_v<std::remove_reference_t<T>>) {
return *obj;
} else {
return std::forward<T>(obj);
}
}

inline void* alloc(int64_t sz) noexcept { return std::malloc(sz); }
inline void goraw_panic(const char* msg) noexcept {
std::fprintf(stderr, "[GORAW PANIC] %s\n", msg);
std::abort();
}

// Встроенные функции и математика Goraw из std
using std::abs;
using std::min;
using std::max;
using std::clamp;
using std::sqrt;
using std::pow;
using std::floor;
using std::ceil;
using std::sin;
using std::cos;

template <typename... Args>
inline void println(const Args&... args) {
auto print_one = [](const auto& val) {
if constexpr (std::is_same_v<std::decay_t<decltype(val)>, GorawStr>) {
std::cout << val.view();
} else {
std::cout << val;
}
};
(print_one(args), ...);
std::cout << std::endl;
}

template <typename... Args>
inline void print(const Args&... args) {
auto print_one = [](const auto& val) {
if constexpr (std::is_same_v<std::decay_t<decltype(val)>, GorawStr>) {
std::cout << val.view();
} else {
std::cout << val;
}
};
(print_one(args), ...);
}

// --- Предварительные объявления структур ---
struct Point;
struct Edge;
struct BufferGuard;

// --- Определения структур ---
struct Point {
    int64_t x{};
    int64_t y{};
    int64_t id{};
};

struct Edge {
    int64_t u{};
    int64_t v{};
    int64_t w{};
};

struct BufferGuard {
    uint8_t* ptr{};
    ~BufferGuard() noexcept;
};

// --- Константы ---
constexpr int64_t COORD_MAX = 1000000000;
constexpr int64_t COORD_MIN = (-1000000000);
constexpr int64_t INF_DIST = 4000000000000000000LL;

// --- Предварительные объявления функций ---
BufferGuard BufferGuard__new(int64_t sz);
void BufferGuard__drop(BufferGuard* self);
int64_t abs_i64(int64_t x);
bool validate_coordinates(int64_t* orig_x, int64_t* orig_y, int64_t n);
bool point_greater(int64_t p1_x, int64_t p1_y, int64_t p2_x, int64_t p2_y);
int64_t calc_max_depth(int64_t n);
void insertion_sort_points(Point* pts, int64_t left, int64_t right);
void heap_sift_down_points(Point* pts, int64_t left, int64_t root, int64_t n);
void heapsort_points(Point* pts, int64_t left, int64_t right);
void introsort_points(Point* pts, int64_t left, int64_t right, int64_t max_depth);
void sort_points(Point* pts, int64_t left, int64_t right);
void insertion_sort_i64(int64_t* arr, int64_t left, int64_t right);
void heap_sift_down_i64(int64_t* arr, int64_t left, int64_t root, int64_t n);
void heapsort_i64(int64_t* arr, int64_t left, int64_t right);
void introsort_i64(int64_t* arr, int64_t left, int64_t right, int64_t max_depth);
void sort_i64(int64_t* arr, int64_t left, int64_t right);
void insertion_sort_edges(Edge* edges, int64_t left, int64_t right);
void heap_sift_down_edges(Edge* edges, int64_t left, int64_t root, int64_t n);
void heapsort_edges(Edge* edges, int64_t left, int64_t right);
void introsort_edges(Edge* edges, int64_t left, int64_t right, int64_t max_depth);
void sort_edges(Edge* edges, int64_t left, int64_t right);
int64_t lower_bound(int64_t* arr, int64_t len, int64_t val);
int64_t dsu_find(int64_t* parent, int64_t x);
bool dsu_union(int64_t* parent, int64_t* rank, int64_t x, int64_t y);
int64_t solve_manhattan_mst_core(int64_t* orig_x, int64_t* orig_y, int64_t n, bool use_pure_heap);
int64_t solve_manhattan_mst(int64_t* orig_x, int64_t* orig_y, int64_t n);
int64_t solve_manhattan_mst_pure_heapsort(int64_t* orig_x, int64_t* orig_y, int64_t n);
int64_t solve_manhattan_mst_bruteforce(int64_t* orig_x, int64_t* orig_y, int64_t n);
void benchmark_100k();
int32_t main();

// --- Определения функций ---
BufferGuard BufferGuard__new(int64_t sz) {
    auto p = ((uint8_t*)(alloc(sz)));
    return BufferGuard{
        .ptr = p
    };
}

void BufferGuard__drop(BufferGuard* self) {
    if (gw_deref(self).ptr != nullptr) {
        free(gw_deref(self).ptr);
        gw_deref(self).ptr = nullptr;
    }
}

int64_t abs_i64(int64_t x) {
    if (x >= 0) {
        return x;
    }
    if (x == ((-9223372036854775807LL) - 1)) {
        return 9223372036854775807LL;
    }
    return (-x);
}

bool validate_coordinates(int64_t* orig_x, int64_t* orig_y, int64_t n) {
    { // unsafe
        for (auto i = 0; (i < n); i = (i + 1)) {
            if ((orig_x[i] < COORD_MIN) || (orig_x[i] > COORD_MAX)) {
                return false;
            }
            if ((orig_y[i] < COORD_MIN) || (orig_y[i] > COORD_MAX)) {
                return false;
            }
        }
    }
    return true;
}

bool point_greater(int64_t p1_x, int64_t p1_y, int64_t p2_x, int64_t p2_y) {
    if (p1_x != p2_x) {
        return (p1_x > p2_x);
    }
    return (p1_y > p2_y);
}

int64_t calc_max_depth(int64_t n) {
    int64_t d = 0;
    auto v = n;
    for (; (v > 0); ) {
        d = (d + 1);
        v = (v / 2);
    }
    return (d * 2);
}

void insertion_sort_points(Point* pts, int64_t left, int64_t right) {
    { // unsafe
        for (auto i = (left + 1); (i <= right); i = (i + 1)) {
            auto key = pts[i];
            auto j = (i - 1);
            for (; ((j >= left) && point_greater(gw_deref(key).x, gw_deref(key).y, gw_deref(pts[j]).x, gw_deref(pts[j]).y)); ) {
                pts[(j + 1)] = pts[j];
                j = (j - 1);
            }
            pts[(j + 1)] = key;
        }
    }
}

void heap_sift_down_points(Point* pts, int64_t left, int64_t root, int64_t n) {
    auto curr = root;
    { // unsafe
        for (; ; ) {
            auto smallest = curr;
            auto left_child = ((2 * curr) + 1);
            auto right_child = ((2 * curr) + 2);
            if ((left_child < n) && point_greater(gw_deref(pts[(left + smallest)]).x, gw_deref(pts[(left + smallest)]).y, gw_deref(pts[(left + left_child)]).x, gw_deref(pts[(left + left_child)]).y)) {
                smallest = left_child;
            }
            if ((right_child < n) && point_greater(gw_deref(pts[(left + smallest)]).x, gw_deref(pts[(left + smallest)]).y, gw_deref(pts[(left + right_child)]).x, gw_deref(pts[(left + right_child)]).y)) {
                smallest = right_child;
            }
            if (smallest == curr) {
                break;
            }
            auto tmp = pts[(left + curr)];
            pts[(left + curr)] = pts[(left + smallest)];
            pts[(left + smallest)] = tmp;
            curr = smallest;
        }
    }
}

void heapsort_points(Point* pts, int64_t left, int64_t right) {
    auto n = ((right - left) + 1);
    if (n <= 1) {
        return;
    }
    auto i = ((n / 2) - 1);
    for (; (i >= 0); ) {
        heap_sift_down_points(pts, left, i, n);
        i = (i - 1);
    }
    auto j = (n - 1);
    { // unsafe
        for (; (j > 0); ) {
            auto tmp = pts[left];
            pts[left] = pts[(left + j)];
            pts[(left + j)] = tmp;
            heap_sift_down_points(pts, left, 0, j);
            j = (j - 1);
        }
    }
}

void introsort_points(Point* pts, int64_t left, int64_t right, int64_t max_depth) {
    auto l = left;
    auto r = right;
    auto depth = max_depth;
    for (; ((r - l) > 16); ) {
        if (depth == 0) {
            heapsort_points(pts, l, r);
            return;
        }
        depth = (depth - 1);
        auto mid = (l + ((r - l) / 2));
        { // unsafe
            if (point_greater(gw_deref(pts[mid]).x, gw_deref(pts[mid]).y, gw_deref(pts[l]).x, gw_deref(pts[l]).y)) {
                auto tmp = pts[l];
                pts[l] = pts[mid];
                pts[mid] = tmp;
            }
            if (point_greater(gw_deref(pts[r]).x, gw_deref(pts[r]).y, gw_deref(pts[l]).x, gw_deref(pts[l]).y)) {
                auto tmp = pts[l];
                pts[l] = pts[r];
                pts[r] = tmp;
            }
            if (point_greater(gw_deref(pts[r]).x, gw_deref(pts[r]).y, gw_deref(pts[mid]).x, gw_deref(pts[mid]).y)) {
                auto tmp = pts[mid];
                pts[mid] = pts[r];
                pts[r] = tmp;
            }
            auto piv_x = gw_deref(pts[mid]).x;
            auto piv_y = gw_deref(pts[mid]).y;
            auto i = l;
            auto j = r;
            for (; (i <= j); ) {
                for (; point_greater(gw_deref(pts[i]).x, gw_deref(pts[i]).y, piv_x, piv_y); ) {
                    i = (i + 1);
                }
                for (; point_greater(piv_x, piv_y, gw_deref(pts[j]).x, gw_deref(pts[j]).y); ) {
                    j = (j - 1);
                }
                if (i <= j) {
                    auto tmp = pts[i];
                    pts[i] = pts[j];
                    pts[j] = tmp;
                    i = (i + 1);
                    j = (j - 1);
                }
            }
            if ((j - l) < (r - i)) {
                if (l < j) {
                    introsort_points(pts, l, j, depth);
                }
                l = i;
            } else {
                if (i < r) {
                    introsort_points(pts, i, r, depth);
                }
                r = j;
            }
        }
    }
    insertion_sort_points(pts, l, r);
}

void sort_points(Point* pts, int64_t left, int64_t right) {
    if (left >= right) {
        return;
    }
    auto depth = calc_max_depth(((right - left) + 1));
    introsort_points(pts, left, right, depth);
}

void insertion_sort_i64(int64_t* arr, int64_t left, int64_t right) {
    { // unsafe
        for (auto i = (left + 1); (i <= right); i = (i + 1)) {
            auto key = arr[i];
            auto j = (i - 1);
            for (; ((j >= left) && (arr[j] > key)); ) {
                arr[(j + 1)] = arr[j];
                j = (j - 1);
            }
            arr[(j + 1)] = key;
        }
    }
}

void heap_sift_down_i64(int64_t* arr, int64_t left, int64_t root, int64_t n) {
    auto curr = root;
    { // unsafe
        for (; ; ) {
            auto largest = curr;
            auto left_child = ((2 * curr) + 1);
            auto right_child = ((2 * curr) + 2);
            if ((left_child < n) && (arr[(left + left_child)] > arr[(left + largest)])) {
                largest = left_child;
            }
            if ((right_child < n) && (arr[(left + right_child)] > arr[(left + largest)])) {
                largest = right_child;
            }
            if (largest == curr) {
                break;
            }
            auto tmp = arr[(left + curr)];
            arr[(left + curr)] = arr[(left + largest)];
            arr[(left + largest)] = tmp;
            curr = largest;
        }
    }
}

void heapsort_i64(int64_t* arr, int64_t left, int64_t right) {
    auto n = ((right - left) + 1);
    if (n <= 1) {
        return;
    }
    auto i = ((n / 2) - 1);
    for (; (i >= 0); ) {
        heap_sift_down_i64(arr, left, i, n);
        i = (i - 1);
    }
    auto j = (n - 1);
    { // unsafe
        for (; (j > 0); ) {
            auto tmp = arr[left];
            arr[left] = arr[(left + j)];
            arr[(left + j)] = tmp;
            heap_sift_down_i64(arr, left, 0, j);
            j = (j - 1);
        }
    }
}

void introsort_i64(int64_t* arr, int64_t left, int64_t right, int64_t max_depth) {
    auto l = left;
    auto r = right;
    auto depth = max_depth;
    for (; ((r - l) > 16); ) {
        if (depth == 0) {
            heapsort_i64(arr, l, r);
            return;
        }
        depth = (depth - 1);
        auto mid = (l + ((r - l) / 2));
        { // unsafe
            if (arr[l] > arr[mid]) {
                auto tmp = arr[l];
                arr[l] = arr[mid];
                arr[mid] = tmp;
            }
            if (arr[l] > arr[r]) {
                auto tmp = arr[l];
                arr[l] = arr[r];
                arr[r] = tmp;
            }
            if (arr[mid] > arr[r]) {
                auto tmp = arr[mid];
                arr[mid] = arr[r];
                arr[r] = tmp;
            }
            auto pivot = arr[mid];
            auto i = l;
            auto j = r;
            for (; (i <= j); ) {
                for (; (arr[i] < pivot); ) {
                    i = (i + 1);
                }
                for (; (arr[j] > pivot); ) {
                    j = (j - 1);
                }
                if (i <= j) {
                    auto tmp = arr[i];
                    arr[i] = arr[j];
                    arr[j] = tmp;
                    i = (i + 1);
                    j = (j - 1);
                }
            }
            if ((j - l) < (r - i)) {
                if (l < j) {
                    introsort_i64(arr, l, j, depth);
                }
                l = i;
            } else {
                if (i < r) {
                    introsort_i64(arr, i, r, depth);
                }
                r = j;
            }
        }
    }
    insertion_sort_i64(arr, l, r);
}

void sort_i64(int64_t* arr, int64_t left, int64_t right) {
    if (left >= right) {
        return;
    }
    auto depth = calc_max_depth(((right - left) + 1));
    introsort_i64(arr, left, right, depth);
}

void insertion_sort_edges(Edge* edges, int64_t left, int64_t right) {
    { // unsafe
        for (auto i = (left + 1); (i <= right); i = (i + 1)) {
            auto key = edges[i];
            auto j = (i - 1);
            for (; ((j >= left) && (gw_deref(edges[j]).w > gw_deref(key).w)); ) {
                edges[(j + 1)] = edges[j];
                j = (j - 1);
            }
            edges[(j + 1)] = key;
        }
    }
}

void heap_sift_down_edges(Edge* edges, int64_t left, int64_t root, int64_t n) {
    auto curr = root;
    { // unsafe
        for (; ; ) {
            auto largest = curr;
            auto left_child = ((2 * curr) + 1);
            auto right_child = ((2 * curr) + 2);
            if ((left_child < n) && (gw_deref(edges[(left + left_child)]).w > gw_deref(edges[(left + largest)]).w)) {
                largest = left_child;
            }
            if ((right_child < n) && (gw_deref(edges[(left + right_child)]).w > gw_deref(edges[(left + largest)]).w)) {
                largest = right_child;
            }
            if (largest == curr) {
                break;
            }
            auto tmp = edges[(left + curr)];
            edges[(left + curr)] = edges[(left + largest)];
            edges[(left + largest)] = tmp;
            curr = largest;
        }
    }
}

void heapsort_edges(Edge* edges, int64_t left, int64_t right) {
    auto n = ((right - left) + 1);
    if (n <= 1) {
        return;
    }
    auto i = ((n / 2) - 1);
    for (; (i >= 0); ) {
        heap_sift_down_edges(edges, left, i, n);
        i = (i - 1);
    }
    auto j = (n - 1);
    { // unsafe
        for (; (j > 0); ) {
            auto tmp = edges[left];
            edges[left] = edges[(left + j)];
            edges[(left + j)] = tmp;
            heap_sift_down_edges(edges, left, 0, j);
            j = (j - 1);
        }
    }
}

void introsort_edges(Edge* edges, int64_t left, int64_t right, int64_t max_depth) {
    auto l = left;
    auto r = right;
    auto depth = max_depth;
    for (; ((r - l) > 16); ) {
        if (depth == 0) {
            heapsort_edges(edges, l, r);
            return;
        }
        depth = (depth - 1);
        auto mid = (l + ((r - l) / 2));
        { // unsafe
            if (gw_deref(edges[l]).w > gw_deref(edges[mid]).w) {
                auto tmp = edges[l];
                edges[l] = edges[mid];
                edges[mid] = tmp;
            }
            if (gw_deref(edges[l]).w > gw_deref(edges[r]).w) {
                auto tmp = edges[l];
                edges[l] = edges[r];
                edges[r] = tmp;
            }
            if (gw_deref(edges[mid]).w > gw_deref(edges[r]).w) {
                auto tmp = edges[mid];
                edges[mid] = edges[r];
                edges[r] = tmp;
            }
            auto piv_w = gw_deref(edges[mid]).w;
            auto i = l;
            auto j = r;
            for (; (i <= j); ) {
                for (; (gw_deref(edges[i]).w < piv_w); ) {
                    i = (i + 1);
                }
                for (; (gw_deref(edges[j]).w > piv_w); ) {
                    j = (j - 1);
                }
                if (i <= j) {
                    auto tmp = edges[i];
                    edges[i] = edges[j];
                    edges[j] = tmp;
                    i = (i + 1);
                    j = (j - 1);
                }
            }
            if ((j - l) < (r - i)) {
                if (l < j) {
                    introsort_edges(edges, l, j, depth);
                }
                l = i;
            } else {
                if (i < r) {
                    introsort_edges(edges, i, r, depth);
                }
                r = j;
            }
        }
    }
    insertion_sort_edges(edges, l, r);
}

void sort_edges(Edge* edges, int64_t left, int64_t right) {
    if (left >= right) {
        return;
    }
    auto depth = calc_max_depth(((right - left) + 1));
    introsort_edges(edges, left, right, depth);
}

int64_t lower_bound(int64_t* arr, int64_t len, int64_t val) {
    int64_t low = 0;
    int64_t high = (len - 1);
    int64_t ans = 0;
    { // unsafe
        for (; (low <= high); ) {
            auto mid = (low + ((high - low) / 2));
            if (arr[mid] >= val) {
                ans = mid;
                high = (mid - 1);
            } else {
                low = (mid + 1);
            }
        }
    }
    return ans;
}

int64_t dsu_find(int64_t* parent, int64_t x) {
    { // unsafe
        auto root = x;
        for (; (parent[root] != root); ) {
            root = parent[root];
        }
        auto curr = x;
        for (; (curr != root); ) {
            auto nxt = parent[curr];
            parent[curr] = root;
            curr = nxt;
        }
        return root;
    }
}

bool dsu_union(int64_t* parent, int64_t* rank, int64_t x, int64_t y) {
    auto rx = dsu_find(parent, x);
    auto ry = dsu_find(parent, y);
    if (rx == ry) {
        return false;
    }
    { // unsafe
        if (rank[rx] < rank[ry]) {
            parent[rx] = ry;
        } else {
            if (rank[rx] > rank[ry]) {
                parent[ry] = rx;
            } else {
                parent[ry] = rx;
                rank[rx] = (rank[rx] + 1);
            }
        }
    }
    return true;
}

int64_t solve_manhattan_mst_core(int64_t* orig_x, int64_t* orig_y, int64_t n, bool use_pure_heap) {
    if (n <= 1) {
        return 0;
    }
    if (!validate_coordinates(orig_x, orig_y, n)) {
        printf("[ERROR] Coordinates out of supported range [%lld, %lld]\n", COORD_MIN, COORD_MAX);
        return (-1);
    }
    auto pts_buf = BufferGuard__new((n * sizeof(Point)));
    Point* pts = ((Point*)(gw_deref(pts_buf).ptr));
    auto z_buf = BufferGuard__new((n * sizeof(i64)));
    int64_t* z_vals = ((int64_t*)(gw_deref(z_buf).ptr));
    auto bit_val_buf = BufferGuard__new(((n + 4) * sizeof(i64)));
    int64_t* bit_val = ((int64_t*)(gw_deref(bit_val_buf).ptr));
    auto bit_id_buf = BufferGuard__new(((n + 4) * sizeof(i64)));
    int64_t* bit_id = ((int64_t*)(gw_deref(bit_id_buf).ptr));
    int64_t max_edges = ((4 * n) + 10);
    auto edges_buf = BufferGuard__new((max_edges * sizeof(Edge)));
    Edge* edges = ((Edge*)(gw_deref(edges_buf).ptr));
    int64_t edge_count = 0;
    { // unsafe
        for (auto i = 0; (i < n); i = (i + 1)) {
            pts[i] = Point{
        .x = orig_x[i],
        .y = orig_y[i],
        .id = i
    };
        }
        int64_t inf = INF_DIST;
        for (auto dir = 0; (dir < 4); dir = (dir + 1)) {
            if ((dir == 1) || (dir == 3)) {
                for (auto i = 0; (i < n); i = (i + 1)) {
                    auto tmp = gw_deref(pts[i]).x;
                    gw_deref(pts[i]).x = gw_deref(pts[i]).y;
                    gw_deref(pts[i]).y = tmp;
                }
            } else {
                if (dir == 2) {
                    for (auto i = 0; (i < n); i = (i + 1)) {
                        gw_deref(pts[i]).x = (-gw_deref(pts[i]).x);
                    }
                }
            }
            if (use_pure_heap) {
                heapsort_points(pts, 0, (n - 1));
            } else {
                sort_points(pts, 0, (n - 1));
            }
            for (auto i = 0; (i < n); i = (i + 1)) {
                z_vals[i] = (gw_deref(pts[i]).y - gw_deref(pts[i]).x);
            }
            if (use_pure_heap) {
                heapsort_i64(z_vals, 0, (n - 1));
            } else {
                sort_i64(z_vals, 0, (n - 1));
            }
            int64_t m = 0;
            if (n > 0) {
                m = 1;
                for (auto i = 1; (i < n); i = (i + 1)) {
                    if (z_vals[i] != z_vals[(m - 1)]) {
                        z_vals[m] = z_vals[i];
                        m = (m + 1);
                    }
                }
            }
            for (auto p = 0; (p <= (m + 2)); p = (p + 1)) {
                bit_val[p] = inf;
                bit_id[p] = (-1);
            }
            for (auto i = 0; (i < n); i = (i + 1)) {
                auto z = (gw_deref(pts[i]).y - gw_deref(pts[i]).x);
                auto rank = lower_bound(z_vals, m, z);
                auto pos = (m - rank);
                int64_t best_id = (-1);
                int64_t min_val = inf;
                auto p = pos;
                for (; (p > 0); ) {
                    if (bit_val[p] < min_val) {
                        min_val = bit_val[p];
                        best_id = bit_id[p];
                    }
                    auto lowbit = (p & (-p));
                    p = (p - lowbit);
                }
                if (best_id != (-1)) {
                    auto u = gw_deref(pts[i]).id;
                    auto v = best_id;
                    auto dist = (abs_i64((orig_x[u] - orig_x[v])) + abs_i64((orig_y[u] - orig_y[v])));
                    edges[edge_count] = Edge{
        .u = u,
        .v = v,
        .w = dist
    };
                    edge_count = (edge_count + 1);
                }
                auto val = (gw_deref(pts[i]).x + gw_deref(pts[i]).y);
                auto id = gw_deref(pts[i]).id;
                auto up = pos;
                for (; (up <= m); ) {
                    if (val < bit_val[up]) {
                        bit_val[up] = val;
                        bit_id[up] = id;
                    }
                    auto lowbit = (up & (-up));
                    up = (up + lowbit);
                }
            }
        }
    }
    if (edge_count > 0) {
        if (use_pure_heap) {
            heapsort_edges(edges, 0, (edge_count - 1));
        } else {
            sort_edges(edges, 0, (edge_count - 1));
        }
    }
    auto parent_buf = BufferGuard__new((n * sizeof(i64)));
    int64_t* parent = ((int64_t*)(gw_deref(parent_buf).ptr));
    auto rank_buf = BufferGuard__new((n * sizeof(i64)));
    int64_t* rank = ((int64_t*)(gw_deref(rank_buf).ptr));
    { // unsafe
        for (auto i = 0; (i < n); i = (i + 1)) {
            parent[i] = i;
            rank[i] = 0;
        }
    }
    int64_t total_mst_weight = 0;
    int64_t edges_added = 0;
    { // unsafe
        for (auto i = 0; (i < edge_count); i = (i + 1)) {
            auto e = edges[i];
            if (dsu_union(parent, rank, gw_deref(e).u, gw_deref(e).v)) {
                total_mst_weight = (total_mst_weight + gw_deref(e).w);
                edges_added = (edges_added + 1);
                if (edges_added == (n - 1)) {
                    break;
                }
            }
        }
    }
    return total_mst_weight;
}

int64_t solve_manhattan_mst(int64_t* orig_x, int64_t* orig_y, int64_t n) {
    return solve_manhattan_mst_core(orig_x, orig_y, n, false);
}

int64_t solve_manhattan_mst_pure_heapsort(int64_t* orig_x, int64_t* orig_y, int64_t n) {
    return solve_manhattan_mst_core(orig_x, orig_y, n, true);
}

int64_t solve_manhattan_mst_bruteforce(int64_t* orig_x, int64_t* orig_y, int64_t n) {
    if (n <= 1) {
        return 0;
    }
    auto min_dist_buf = BufferGuard__new((n * sizeof(i64)));
    int64_t* min_dist = ((int64_t*)(gw_deref(min_dist_buf).ptr));
    auto visited_buf = BufferGuard__new((n * sizeof(bool)));
    bool* visited = ((bool*)(gw_deref(visited_buf).ptr));
    int64_t inf = INF_DIST;
    { // unsafe
        for (auto i = 0; (i < n); i = (i + 1)) {
            min_dist[i] = inf;
            visited[i] = false;
        }
        min_dist[0] = 0;
        int64_t total_weight = 0;
        for (auto step = 0; (step < n); step = (step + 1)) {
            int64_t u = (-1);
            int64_t best_d = inf;
            for (auto i = 0; (i < n); i = (i + 1)) {
                if ((!visited[i]) && (min_dist[i] < best_d)) {
                    best_d = min_dist[i];
                    u = i;
                }
            }
            visited[u] = true;
            total_weight = (total_weight + best_d);
            for (auto v = 0; (v < n); v = (v + 1)) {
                if (!visited[v]) {
                    auto d = (abs_i64((orig_x[u] - orig_x[v])) + abs_i64((orig_y[u] - orig_y[v])));
                    if (d < min_dist[v]) {
                        min_dist[v] = d;
                    }
                }
            }
        }
        return total_weight;
    }
}

void benchmark_100k() {
    int64_t n = 100000;
    int64_t* x = ((int64_t*)(alloc((n * sizeof(i64)))));
    int64_t* y = ((int64_t*)(alloc((n * sizeof(i64)))));
    int64_t rng = 987654321;
    { // unsafe
        for (auto i = 0; (i < n); i = (i + 1)) {
            rng = (((rng * 1103515245) + 12345) & 2147483647);
            x[i] = (rng % 100000000);
            rng = (((rng * 1103515245) + 12345) & 2147483647);
            y[i] = (rng % 100000000);
        }
    }
    printf("Running benchmark on N = %lld points (Hard constraint)...\n", n);
    auto t0 = clock();
    auto mst = solve_manhattan_mst(x, y, n);
    auto t1 = clock();
    auto elapsed_ms = (t1 - t0);
    printf("100,000 points MST computed successfully: Total weight = %lld in %lld ms\n", mst, elapsed_ms);
    free(((uint8_t*)(x)));
    free(((uint8_t*)(y)));
}

int32_t main() {
    printf("============================================================\n");
    printf("  Manhattan MST - O(N log N) Algorithm in Goraw   \n");
    printf("============================================================\n\n");
    int64_t n3 = 3;
    int64_t* x3 = ((int64_t*)(alloc((n3 * sizeof(i64)))));
    int64_t* y3 = ((int64_t*)(alloc((n3 * sizeof(i64)))));
    { // unsafe
        x3[0] = 0;
        y3[0] = 0;
        x3[1] = 1;
        y3[1] = 2;
        x3[2] = 2;
        y3[2] = 1;
    }
    auto mst3 = solve_manhattan_mst(x3, y3, n3);
    printf("Sample 3 points: MST = %lld (Expected 5)\n", mst3);
    free(((uint8_t*)(x3)));
    free(((uint8_t*)(y3)));
    int64_t n4 = 4;
    int64_t* x4 = ((int64_t*)(alloc((n4 * sizeof(i64)))));
    int64_t* y4 = ((int64_t*)(alloc((n4 * sizeof(i64)))));
    { // unsafe
        x4[0] = 0;
        y4[0] = 0;
        x4[1] = 0;
        y4[1] = 10;
        x4[2] = 10;
        y4[2] = 0;
        x4[3] = 10;
        y4[3] = 10;
    }
    auto mst4 = solve_manhattan_mst(x4, y4, n4);
    printf("Sample square 4 points: MST = %lld (Expected 30)\n", mst4);
    free(((uint8_t*)(x4)));
    free(((uint8_t*)(y4)));
    benchmark_100k();
    printf("\nDone! Manhattan MST completed successfully.\n");
    return 0;
}

// --- Деструкторы структур (RAII) ---
inline BufferGuard::~BufferGuard() noexcept { BufferGuard__drop(this); }

