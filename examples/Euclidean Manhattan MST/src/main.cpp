// ============================================================================
// Сгенерировано компилятором Goraw (C++23 Backend)
// ============================================================================

#include <cstdint>
#include <cstddef>
#include <cstdlib>
#include <cstdio>
#include <cstring>
#include <cassert>
#include <ctime>

inline void* alloc(int64_t sz) noexcept { return std::malloc(sz); }
inline void goraw_panic(const char* msg) noexcept {
std::fprintf(stderr, "[GORAW PANIC] %s\n", msg);
std::abort();
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

    constexpr Point() = default;
    constexpr Point(int64_t x, int64_t y, int64_t id) : x(x), y(y), id(id) {}
};

struct Edge {
    int64_t u{};
    int64_t v{};
    int64_t w{};

    constexpr Edge() = default;
    constexpr Edge(int64_t u, int64_t v, int64_t w) : u(u), v(v), w(w) {}
};

struct BufferGuard {
    uint8_t* ptr{};

    constexpr BufferGuard() = default;
    constexpr BufferGuard(uint8_t* ptr) : ptr(ptr) {}
    ~BufferGuard() noexcept;
    BufferGuard(const BufferGuard&) = delete;
    BufferGuard& operator=(const BufferGuard&) = delete;
    constexpr BufferGuard(BufferGuard&& other) noexcept : ptr(other.ptr) {
        other.ptr = nullptr;
    }
    BufferGuard& operator=(BufferGuard&& other) noexcept;
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
namespace contracts { void run_all_contracts(); }
namespace integration_tests { void run_all_tests(); }
int32_t run_all_goraw_tests();

// --- Определения функций ---
BufferGuard BufferGuard__new(int64_t sz) {
    auto p = ((uint8_t*)(alloc(sz)));
    return BufferGuard(p);
}

void BufferGuard__drop(BufferGuard* self) {
    if (self->ptr != nullptr) {
        free(self->ptr);
        self->ptr = nullptr;
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
            for (; ((j >= left) && point_greater(key.x, key.y, pts[j].x, pts[j].y)); ) {
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
            if ((left_child < n) && point_greater(pts[(left + smallest)].x, pts[(left + smallest)].y, pts[(left + left_child)].x, pts[(left + left_child)].y)) {
                smallest = left_child;
            }
            if ((right_child < n) && point_greater(pts[(left + smallest)].x, pts[(left + smallest)].y, pts[(left + right_child)].x, pts[(left + right_child)].y)) {
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
            if (point_greater(pts[mid].x, pts[mid].y, pts[l].x, pts[l].y)) {
                auto tmp = pts[l];
                pts[l] = pts[mid];
                pts[mid] = tmp;
            }
            if (point_greater(pts[r].x, pts[r].y, pts[l].x, pts[l].y)) {
                auto tmp = pts[l];
                pts[l] = pts[r];
                pts[r] = tmp;
            }
            if (point_greater(pts[r].x, pts[r].y, pts[mid].x, pts[mid].y)) {
                auto tmp = pts[mid];
                pts[mid] = pts[r];
                pts[r] = tmp;
            }
            auto piv_x = pts[mid].x;
            auto piv_y = pts[mid].y;
            auto i = l;
            auto j = r;
            for (; (i <= j); ) {
                for (; point_greater(pts[i].x, pts[i].y, piv_x, piv_y); ) {
                    i = (i + 1);
                }
                for (; point_greater(piv_x, piv_y, pts[j].x, pts[j].y); ) {
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
            for (; ((j >= left) && (edges[j].w > key.w)); ) {
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
            if ((left_child < n) && (edges[(left + left_child)].w > edges[(left + largest)].w)) {
                largest = left_child;
            }
            if ((right_child < n) && (edges[(left + right_child)].w > edges[(left + largest)].w)) {
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
            if (edges[l].w > edges[mid].w) {
                auto tmp = edges[l];
                edges[l] = edges[mid];
                edges[mid] = tmp;
            }
            if (edges[l].w > edges[r].w) {
                auto tmp = edges[l];
                edges[l] = edges[r];
                edges[r] = tmp;
            }
            if (edges[mid].w > edges[r].w) {
                auto tmp = edges[mid];
                edges[mid] = edges[r];
                edges[r] = tmp;
            }
            auto piv_w = edges[mid].w;
            auto i = l;
            auto j = r;
            for (; (i <= j); ) {
                for (; (edges[i].w < piv_w); ) {
                    i = (i + 1);
                }
                for (; (edges[j].w > piv_w); ) {
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
    Point* pts = ((Point*)(pts_buf.ptr));
    auto z_buf = BufferGuard__new((n * sizeof(int64_t)));
    int64_t* z_vals = ((int64_t*)(z_buf.ptr));
    auto bit_val_buf = BufferGuard__new(((n + 4) * sizeof(int64_t)));
    int64_t* bit_val = ((int64_t*)(bit_val_buf.ptr));
    auto bit_id_buf = BufferGuard__new(((n + 4) * sizeof(int64_t)));
    int64_t* bit_id = ((int64_t*)(bit_id_buf.ptr));
    int64_t max_edges = ((4 * n) + 10);
    auto edges_buf = BufferGuard__new((max_edges * sizeof(Edge)));
    Edge* edges = ((Edge*)(edges_buf.ptr));
    int64_t edge_count = 0;
    { // unsafe
        for (auto i = 0; (i < n); i = (i + 1)) {
            pts[i] = Point(orig_x[i], orig_y[i], i);
        }
        int64_t inf = INF_DIST;
        for (auto dir = 0; (dir < 4); dir = (dir + 1)) {
            if ((dir == 1) || (dir == 3)) {
                for (auto i = 0; (i < n); i = (i + 1)) {
                    auto tmp = pts[i].x;
                    pts[i].x = pts[i].y;
                    pts[i].y = tmp;
                }
            } else {
                if (dir == 2) {
                    for (auto i = 0; (i < n); i = (i + 1)) {
                        pts[i].x = (-pts[i].x);
                    }
                }
            }
            if (use_pure_heap) {
                heapsort_points(pts, 0, (n - 1));
            } else {
                sort_points(pts, 0, (n - 1));
            }
            for (auto i = 0; (i < n); i = (i + 1)) {
                z_vals[i] = (pts[i].y - pts[i].x);
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
                auto z = (pts[i].y - pts[i].x);
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
                    auto u = pts[i].id;
                    auto v = best_id;
                    auto dist = (abs_i64((orig_x[u] - orig_x[v])) + abs_i64((orig_y[u] - orig_y[v])));
                    edges[edge_count] = Edge(u, v, dist);
                    edge_count = (edge_count + 1);
                }
                auto val = (pts[i].x + pts[i].y);
                auto id = pts[i].id;
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
    auto parent_buf = BufferGuard__new((n * sizeof(int64_t)));
    int64_t* parent = ((int64_t*)(parent_buf.ptr));
    auto rank_buf = BufferGuard__new((n * sizeof(int64_t)));
    int64_t* rank = ((int64_t*)(rank_buf.ptr));
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
            if (dsu_union(parent, rank, e.u, e.v)) {
                total_mst_weight = (total_mst_weight + e.w);
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
    auto min_dist_buf = BufferGuard__new((n * sizeof(int64_t)));
    int64_t* min_dist = ((int64_t*)(min_dist_buf.ptr));
    auto visited_buf = BufferGuard__new((n * sizeof(bool)));
    bool* visited = ((bool*)(visited_buf.ptr));
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
    int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
    int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
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

int32_t main(int argc, char** argv) {
    #ifdef GORAW_TEST
    return run_all_goraw_tests();
    #else
    if (argc > 1 && (std::strcmp(argv[1], "--test") == 0 || std::strcmp(argv[1], "-t") == 0)) {
        return run_all_goraw_tests();
    }
    #endif
    printf("============================================================\n");
    printf("  Manhattan MST - O(N log N) Algorithm in Goraw   \n");
    printf("============================================================\n\n");
    int64_t n3 = 3;
    int64_t* x3 = ((int64_t*)(alloc((n3 * sizeof(int64_t)))));
    int64_t* y3 = ((int64_t*)(alloc((n3 * sizeof(int64_t)))));
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
    int64_t* x4 = ((int64_t*)(alloc((n4 * sizeof(int64_t)))));
    int64_t* y4 = ((int64_t*)(alloc((n4 * sizeof(int64_t)))));
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

// --- Деструкторы и операторы перемещения структур (RAII) ---
inline BufferGuard::~BufferGuard() noexcept {
    BufferGuard__drop(this);
}
inline BufferGuard& BufferGuard::operator=(BufferGuard&& other) noexcept {
    if (this != &other) {
        BufferGuard__drop(this);
        ptr = other.ptr;
        other.ptr = nullptr;
    }
    return *this;
}


// --- Shadow-контракты и интеграционные тесты ---
namespace contracts {
    inline void contract_abs_i64() {
        assert((abs_i64(10) == 10));
        assert((abs_i64((-42)) == 42));
        assert((abs_i64(0) == 0));
        assert((abs_i64(((-9223372036854775807LL) - 1)) == 9223372036854775807LL));
    }

    inline void contract_validate_coordinates() {
        int64_t n = 2;
        auto x_buf = BufferGuard__new((n * sizeof(int64_t)));
        auto y_buf = BufferGuard__new((n * sizeof(int64_t)));
        auto x = ((int64_t*)(x_buf.ptr));
        auto y = ((int64_t*)(y_buf.ptr));
        { // unsafe
            x[0] = 500;
            y[0] = (-500);
            x[1] = COORD_MAX;
            y[1] = COORD_MIN;
        }
        assert((validate_coordinates(x, y, n) == true));
        { // unsafe
            x[1] = (COORD_MAX + 1);
        }
        assert((validate_coordinates(x, y, n) == false));
    }

    inline void contract_point_greater() {
        assert((point_greater(5, 2, 3, 10) == true));
        assert((point_greater(3, 10, 5, 2) == false));
        assert((point_greater(4, 7, 4, 3) == true));
        assert((point_greater(4, 3, 4, 7) == false));
        assert((point_greater(4, 3, 4, 3) == false));
    }

    inline void run_all_contracts() {
        std::printf("\n--- Shadow Contracts ---\n");
        contract_abs_i64();
        std::printf("[CONTRACT ok] abs_i64\n");
        contract_validate_coordinates();
        std::printf("[CONTRACT ok] validate_coordinates\n");
        contract_point_greater();
        std::printf("[CONTRACT ok] point_greater\n");
    }

} // namespace contracts

namespace integration_tests {
    inline void test_sample_3_points() {
        int64_t n = 3;
        int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        { // unsafe
            x[0] = 0;
            y[0] = 0;
            x[1] = 1;
            y[1] = 2;
            x[2] = 2;
            y[2] = 1;
        }
        auto mst = solve_manhattan_mst(x, y, n);
        auto bf = solve_manhattan_mst_bruteforce(x, y, n);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == 5));
        assert((bf == 5));
    }

    inline void test_sample_square() {
        int64_t n = 4;
        int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        { // unsafe
            x[0] = 0;
            y[0] = 0;
            x[1] = 0;
            y[1] = 10;
            x[2] = 10;
            y[2] = 0;
            x[3] = 10;
            y[3] = 10;
        }
        auto mst = solve_manhattan_mst(x, y, n);
        auto bf = solve_manhattan_mst_bruteforce(x, y, n);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == 30));
        assert((bf == 30));
    }

    inline void test_stress_random_100() {
        int64_t count = 100;
        int64_t* x = ((int64_t*)(alloc((count * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((count * sizeof(int64_t)))));
        int64_t rng = 123456789;
        { // unsafe
            for (auto i = 0; (i < count); i = (i + 1)) {
                rng = (((rng * 1103515245) + 12345) & 2147483647);
                x[i] = (rng % 10000);
                rng = (((rng * 1103515245) + 12345) & 2147483647);
                y[i] = (rng % 10000);
            }
        }
        auto mst = solve_manhattan_mst(x, y, count);
        auto bf = solve_manhattan_mst_bruteforce(x, y, count);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == bf));
    }

    inline void test_collinear_horizontal() {
        int64_t n = 6;
        int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        { // unsafe
            x[0] = 10;
            y[0] = 5;
            x[1] = 2;
            y[1] = 5;
            x[2] = 7;
            y[2] = 5;
            x[3] = 15;
            y[3] = 5;
            x[4] = 0;
            y[4] = 5;
            x[5] = 4;
            y[5] = 5;
        }
        auto mst = solve_manhattan_mst(x, y, n);
        auto bf = solve_manhattan_mst_bruteforce(x, y, n);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == 15));
        assert((bf == 15));
    }

    inline void test_collinear_vertical() {
        int64_t n = 6;
        int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        { // unsafe
            x[0] = (-3);
            y[0] = 10;
            x[1] = (-3);
            y[1] = 2;
            x[2] = (-3);
            y[2] = 7;
            x[3] = (-3);
            y[3] = 15;
            x[4] = (-3);
            y[4] = 0;
            x[5] = (-3);
            y[5] = 4;
        }
        auto mst = solve_manhattan_mst(x, y, n);
        auto bf = solve_manhattan_mst_bruteforce(x, y, n);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == 15));
        assert((bf == 15));
    }

    inline void test_collinear_diagonal_pos() {
        int64_t n = 5;
        int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        { // unsafe
            x[0] = 1;
            y[0] = 1;
            x[1] = 5;
            y[1] = 5;
            x[2] = 2;
            y[2] = 2;
            x[3] = 4;
            y[3] = 4;
            x[4] = 3;
            y[4] = 3;
        }
        auto mst = solve_manhattan_mst(x, y, n);
        auto bf = solve_manhattan_mst_bruteforce(x, y, n);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == 8));
        assert((bf == 8));
    }

    inline void test_collinear_diagonal_neg() {
        int64_t n = 5;
        int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        { // unsafe
            x[0] = 1;
            y[0] = (-1);
            x[1] = 5;
            y[1] = (-5);
            x[2] = 2;
            y[2] = (-2);
            x[3] = 4;
            y[3] = (-4);
            x[4] = 3;
            y[4] = (-3);
        }
        auto mst = solve_manhattan_mst(x, y, n);
        auto bf = solve_manhattan_mst_bruteforce(x, y, n);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == 8));
        assert((bf == 8));
    }

    inline void test_grid_4x4() {
        int64_t n = 16;
        int64_t* x = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((n * sizeof(int64_t)))));
        { // unsafe
            int64_t idx = 0;
            for (auto r = 0; (r < 4); r = (r + 1)) {
                for (auto c = 0; (c < 4); c = (c + 1)) {
                    x[idx] = (c * 10);
                    y[idx] = (r * 10);
                    idx = (idx + 1);
                }
            }
        }
        auto mst = solve_manhattan_mst(x, y, n);
        auto bf = solve_manhattan_mst_bruteforce(x, y, n);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == 150));
        assert((bf == 150));
    }

    inline void test_stress_collinear_random() {
        int64_t count = 80;
        int64_t* x = ((int64_t*)(alloc((count * sizeof(int64_t)))));
        int64_t* y = ((int64_t*)(alloc((count * sizeof(int64_t)))));
        int64_t rng = 99991;
        { // unsafe
            for (auto i = 0; (i < count); i = (i + 1)) {
                rng = (((rng * 1103515245) + 12345) & 2147483647);
                auto coord = (rng % 500);
                if ((i % 3) == 0) {
                    x[i] = coord;
                    y[i] = 100;
                } else {
                    if ((i % 3) == 1) {
                        x[i] = 200;
                        y[i] = coord;
                    } else {
                        x[i] = coord;
                        y[i] = (-coord);
                    }
                }
            }
        }
        auto mst = solve_manhattan_mst(x, y, count);
        auto bf = solve_manhattan_mst_bruteforce(x, y, count);
        free(((uint8_t*)(x)));
        free(((uint8_t*)(y)));
        assert((mst == bf));
    }

    inline void test_introsort_adversarial_patterns() {
        int64_t n = 10000;
        auto buf = BufferGuard__new((n * sizeof(int64_t)));
        auto arr = ((int64_t*)(buf.ptr));
        { // unsafe
            for (auto i = 0; (i < n); i = (i + 1)) {
                arr[i] = i;
            }
        }
        sort_i64(arr, 0, (n - 1));
        { // unsafe
            for (auto i = 0; (i < (n - 1)); i = (i + 1)) {
                assert((arr[i] <= arr[(i + 1)]));
            }
            assert((arr[0] == 0));
            assert((arr[(n - 1)] == (n - 1)));
        }
        { // unsafe
            for (auto i = 0; (i < n); i = (i + 1)) {
                arr[i] = (n - i);
            }
        }
        sort_i64(arr, 0, (n - 1));
        { // unsafe
            for (auto i = 0; (i < (n - 1)); i = (i + 1)) {
                assert((arr[i] <= arr[(i + 1)]));
            }
            assert((arr[0] == 1));
            assert((arr[(n - 1)] == n));
        }
        auto half = (n / 2);
        { // unsafe
            for (auto i = 0; (i < half); i = (i + 1)) {
                arr[i] = i;
                arr[((n - 1) - i)] = i;
            }
        }
        sort_i64(arr, 0, (n - 1));
        { // unsafe
            for (auto i = 0; (i < (n - 1)); i = (i + 1)) {
                assert((arr[i] <= arr[(i + 1)]));
            }
            assert((arr[0] == 0));
            assert((arr[1] == 0));
            assert((arr[(n - 1)] == (half - 1)));
        }
        { // unsafe
            for (auto i = 0; (i < n); i = (i + 1)) {
                arr[i] = 42;
            }
        }
        sort_i64(arr, 0, (n - 1));
        { // unsafe
            for (auto i = 0; (i < n); i = (i + 1)) {
                assert((arr[i] == 42));
            }
        }
    }

    inline void test_heapsort_points_direct_verification() {
        int64_t n = 200;
        auto buf = BufferGuard__new((n * sizeof(Point)));
        auto pts = ((Point*)(buf.ptr));
        int64_t rng = 54321;
        { // unsafe
            for (auto i = 0; (i < n); i = (i + 1)) {
                rng = (((rng * 1103515245) + 12345) & 2147483647);
                auto rx = (rng % 1000);
                rng = (((rng * 1103515245) + 12345) & 2147483647);
                auto ry = (rng % 1000);
                pts[i] = Point(rx, ry, i);
            }
        }
        heapsort_points(pts, 0, (n - 1));
        { // unsafe
            for (auto i = 0; (i < (n - 1)); i = (i + 1)) {
                auto next_is_greater = point_greater(pts[(i + 1)].x, pts[(i + 1)].y, pts[i].x, pts[i].y);
                assert((!next_is_greater));
            }
        }
        heapsort_points(pts, 20, 150);
        { // unsafe
            for (auto i = 20; (i < 150); i = (i + 1)) {
                auto next_is_greater = point_greater(pts[(i + 1)].x, pts[(i + 1)].y, pts[i].x, pts[i].y);
                assert((!next_is_greater));
            }
        }
    }

    inline void test_forced_pure_heapsort_mst() {
        int64_t count = 150;
        auto x_buf = BufferGuard__new((count * sizeof(int64_t)));
        auto y_buf = BufferGuard__new((count * sizeof(int64_t)));
        auto x = ((int64_t*)(x_buf.ptr));
        auto y = ((int64_t*)(y_buf.ptr));
        int64_t rng = 13579;
        { // unsafe
            for (auto i = 0; (i < count); i = (i + 1)) {
                rng = (((rng * 1103515245) + 12345) & 2147483647);
                x[i] = (rng % 5000);
                rng = (((rng * 1103515245) + 12345) & 2147483647);
                y[i] = (rng % 5000);
            }
        }
        auto intro_mst = solve_manhattan_mst(x, y, count);
        auto heap_mst = solve_manhattan_mst_pure_heapsort(x, y, count);
        auto bf_mst = solve_manhattan_mst_bruteforce(x, y, count);
        assert((heap_mst == bf_mst));
        assert((intro_mst == heap_mst));
    }

    inline void run_all_tests() {
        std::printf("\n--- Integration Tests ---\n");
        test_sample_3_points();
        std::printf("[TEST ok] sample_3_points\n");
        test_sample_square();
        std::printf("[TEST ok] sample_square\n");
        test_stress_random_100();
        std::printf("[TEST ok] stress_random_100\n");
        test_collinear_horizontal();
        std::printf("[TEST ok] collinear_horizontal\n");
        test_collinear_vertical();
        std::printf("[TEST ok] collinear_vertical\n");
        test_collinear_diagonal_pos();
        std::printf("[TEST ok] collinear_diagonal_pos\n");
        test_collinear_diagonal_neg();
        std::printf("[TEST ok] collinear_diagonal_neg\n");
        test_grid_4x4();
        std::printf("[TEST ok] grid_4x4\n");
        test_stress_collinear_random();
        std::printf("[TEST ok] stress_collinear_random\n");
        test_introsort_adversarial_patterns();
        std::printf("[TEST ok] introsort_adversarial_patterns\n");
        test_heapsort_points_direct_verification();
        std::printf("[TEST ok] heapsort_points_direct_verification\n");
        test_forced_pure_heapsort_mst();
        std::printf("[TEST ok] forced_pure_heapsort_mst\n");
    }

} // namespace integration_tests

inline int32_t run_all_goraw_tests() {
    std::printf("============================================================\n");
    std::printf("  Running Goraw Contracts & Tests in C++23                 \n");
    std::printf("============================================================\n");
    contracts::run_all_contracts();
    integration_tests::run_all_tests();
    std::printf("\n[C++23] All 15 tests PASSED successfully!\n\n");
    return 0;
}

