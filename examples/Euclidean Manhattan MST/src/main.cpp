// Manhattan MST solver in C++23
// Direct idiomatic translation from Goraw (main.gw)
// Complexity: O(N log N) time, O(N) space

#include <vector>
#include <algorithm>
#include <ranges>
#include <numeric>
#include <cstdint>
#include <cstdio>
#include <chrono>
#include <cassert>
#include <utility>

// ============================================================================
// 1. Constants & Data Structures
// ============================================================================

constexpr int64_t COORD_MAX = 1000000000LL;          // +10^9 safe coordinate boundary
constexpr int64_t COORD_MIN = -1000000000LL;         // -10^9 safe coordinate boundary
constexpr int64_t INF_DIST  = 4000000000000000000LL; // 4 * 10^18 sentinel (safe in i64)

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

// Safe abs protecting against 2's complement overflow on INT64_MIN
inline int64_t abs_i64(int64_t x) noexcept {
    if (x >= 0) return x;
    if (x == -9223372036854775807LL - 1LL) {
        return 9223372036854775807LL;
    }
    return -x;
}

inline bool validate_coordinates(const int64_t* orig_x, const int64_t* orig_y, int64_t n) noexcept {
    for (int64_t i = 0; i < n; ++i) {
        if (orig_x[i] < COORD_MIN || orig_x[i] > COORD_MAX) return false;
        if (orig_y[i] < COORD_MIN || orig_y[i] > COORD_MAX) return false;
    }
    return true;
}

// Point comparator: descending by X, then descending by Y
inline bool point_greater(const Point& p1, const Point& p2) noexcept {
    if (p1.x != p2.x) return p1.x > p2.x;
    return p1.y > p2.y;
}

// ============================================================================
// 2. Sorting Engines: Hybrid IntroSort & HeapSort
// ============================================================================

inline void heap_sift_down_points(Point* pts, int64_t left, int64_t root, int64_t n) noexcept {
    int64_t curr = root;
    while (2 * curr + 1 < n) {
        int64_t child = 2 * curr + 1;
        if (child + 1 < n && point_greater(pts[left + child], pts[left + child + 1])) {
            child = child + 1;
        }
        if (point_greater(pts[left + curr], pts[left + child])) {
            std::swap(pts[left + curr], pts[left + child]);
            curr = child;
        } else {
            break;
        }
    }
}

inline void heapsort_points(Point* pts, int64_t left, int64_t right) noexcept {
    int64_t n = right - left + 1;
    if (n <= 1) return;
    for (int64_t i = n / 2 - 1; i >= 0; --i) {
        heap_sift_down_points(pts, left, i, n);
    }
    for (int64_t j = n - 1; j > 0; --j) {
        std::swap(pts[left], pts[left + j]);
        heap_sift_down_points(pts, left, 0, j);
    }
}

inline void insertion_sort_points(Point* pts, int64_t left, int64_t right) noexcept {
    for (int64_t i = left + 1; i <= right; ++i) {
        Point key = pts[i];
        int64_t j = i - 1;
        while (j >= left && !point_greater(pts[j], key)) {
            pts[j + 1] = pts[j];
            --j;
        }
        pts[j + 1] = key;
    }
}

inline void sort_points_rec(Point* pts, int64_t left, int64_t right, int64_t depth_limit) noexcept {
    while (left < right) {
        int64_t n = right - left + 1;
        if (n <= 16) {
            insertion_sort_points(pts, left, right);
            return;
        }
        if (depth_limit == 0) {
            heapsort_points(pts, left, right);
            return;
        }
        --depth_limit;

        int64_t mid = left + (right - left) / 2;
        if (point_greater(pts[mid], pts[left])) std::swap(pts[left], pts[mid]);
        if (point_greater(pts[right], pts[left])) std::swap(pts[left], pts[right]);
        if (point_greater(pts[right], pts[mid])) std::swap(pts[mid], pts[right]);

        Point pivot = pts[mid];
        int64_t i = left;
        int64_t j = right;
        while (i <= j) {
            while (point_greater(pts[i], pivot)) ++i;
            while (point_greater(pivot, pts[j])) --j;
            if (i <= j) {
                std::swap(pts[i], pts[j]);
                ++i;
                --j;
            }
        }

        if (j - left < right - i) {
            if (left < j) sort_points_rec(pts, left, j, depth_limit);
            left = i;
        } else {
            if (i < right) sort_points_rec(pts, i, right, depth_limit);
            right = j;
        }
    }
}

inline void sort_points(Point* pts, int64_t left, int64_t right) noexcept {
    if (left >= right) return;
    int64_t n = right - left + 1;
    int64_t depth = 0;
    while (n > 1) {
        depth += 2;
        n >>= 1;
    }
    sort_points_rec(pts, left, right, depth);
}

// ----------------------------------------------------------------------------
// i64 Sorting
// ----------------------------------------------------------------------------

inline void heap_sift_down_i64(int64_t* arr, int64_t left, int64_t root, int64_t n) noexcept {
    int64_t curr = root;
    while (2 * curr + 1 < n) {
        int64_t child = 2 * curr + 1;
        if (child + 1 < n && arr[left + child] < arr[left + child + 1]) {
            child = child + 1;
        }
        if (arr[left + curr] < arr[left + child]) {
            std::swap(arr[left + curr], arr[left + child]);
            curr = child;
        } else {
            break;
        }
    }
}

inline void heapsort_i64(int64_t* arr, int64_t left, int64_t right) noexcept {
    int64_t n = right - left + 1;
    if (n <= 1) return;
    for (int64_t i = n / 2 - 1; i >= 0; --i) {
        heap_sift_down_i64(arr, left, i, n);
    }
    for (int64_t j = n - 1; j > 0; --j) {
        std::swap(arr[left], arr[left + j]);
        heap_sift_down_i64(arr, left, 0, j);
    }
}

inline void insertion_sort_i64(int64_t* arr, int64_t left, int64_t right) noexcept {
    for (int64_t i = left + 1; i <= right; ++i) {
        int64_t key = arr[i];
        int64_t j = i - 1;
        while (j >= left && arr[j] > key) {
            arr[j + 1] = arr[j];
            --j;
        }
        arr[j + 1] = key;
    }
}

inline void sort_i64_rec(int64_t* arr, int64_t left, int64_t right, int64_t depth_limit) noexcept {
    while (left < right) {
        int64_t n = right - left + 1;
        if (n <= 16) {
            insertion_sort_i64(arr, left, right);
            return;
        }
        if (depth_limit == 0) {
            heapsort_i64(arr, left, right);
            return;
        }
        --depth_limit;

        int64_t mid = left + (right - left) / 2;
        if (arr[left] > arr[mid]) std::swap(arr[left], arr[mid]);
        if (arr[left] > arr[right]) std::swap(arr[left], arr[right]);
        if (arr[mid] > arr[right]) std::swap(arr[mid], arr[right]);

        int64_t pivot = arr[mid];
        int64_t i = left;
        int64_t j = right;
        while (i <= j) {
            while (arr[i] < pivot) ++i;
            while (pivot < arr[j]) --j;
            if (i <= j) {
                std::swap(arr[i], arr[j]);
                ++i;
                --j;
            }
        }

        if (j - left < right - i) {
            if (left < j) sort_i64_rec(arr, left, j, depth_limit);
            left = i;
        } else {
            if (i < right) sort_i64_rec(arr, i, right, depth_limit);
            right = j;
        }
    }
}

inline void sort_i64(int64_t* arr, int64_t left, int64_t right) noexcept {
    if (left >= right) return;
    int64_t n = right - left + 1;
    int64_t depth = 0;
    while (n > 1) {
        depth += 2;
        n >>= 1;
    }
    sort_i64_rec(arr, left, right, depth);
}

// ----------------------------------------------------------------------------
// Edge Sorting
// ----------------------------------------------------------------------------

inline void heap_sift_down_edges(Edge* edges, int64_t left, int64_t root, int64_t n) noexcept {
    int64_t curr = root;
    while (2 * curr + 1 < n) {
        int64_t child = 2 * curr + 1;
        if (child + 1 < n && edges[left + child].w < edges[left + child + 1].w) {
            child = child + 1;
        }
        if (edges[left + curr].w < edges[left + child].w) {
            std::swap(edges[left + curr], edges[left + child]);
            curr = child;
        } else {
            break;
        }
    }
}

inline void heapsort_edges(Edge* edges, int64_t left, int64_t right) noexcept {
    int64_t n = right - left + 1;
    if (n <= 1) return;
    for (int64_t i = n / 2 - 1; i >= 0; --i) {
        heap_sift_down_edges(edges, left, i, n);
    }
    for (int64_t j = n - 1; j > 0; --j) {
        std::swap(edges[left], edges[left + j]);
        heap_sift_down_edges(edges, left, 0, j);
    }
}

inline void insertion_sort_edges(Edge* edges, int64_t left, int64_t right) noexcept {
    for (int64_t i = left + 1; i <= right; ++i) {
        Edge key = edges[i];
        int64_t j = i - 1;
        while (j >= left && edges[j].w > key.w) {
            edges[j + 1] = edges[j];
            --j;
        }
        edges[j + 1] = key;
    }
}

inline void sort_edges_rec(Edge* edges, int64_t left, int64_t right, int64_t depth_limit) noexcept {
    while (left < right) {
        int64_t n = right - left + 1;
        if (n <= 16) {
            insertion_sort_edges(edges, left, right);
            return;
        }
        if (depth_limit == 0) {
            heapsort_edges(edges, left, right);
            return;
        }
        --depth_limit;

        int64_t mid = left + (right - left) / 2;
        if (edges[left].w > edges[mid].w) std::swap(edges[left], edges[mid]);
        if (edges[left].w > edges[right].w) std::swap(edges[left], edges[right]);
        if (edges[mid].w > edges[right].w) std::swap(edges[mid], edges[right]);

        int64_t pivot_w = edges[mid].w;
        int64_t i = left;
        int64_t j = right;
        while (i <= j) {
            while (edges[i].w < pivot_w) ++i;
            while (pivot_w < edges[j].w) --j;
            if (i <= j) {
                std::swap(edges[i], edges[j]);
                ++i;
                --j;
            }
        }

        if (j - left < right - i) {
            if (left < j) sort_edges_rec(edges, left, j, depth_limit);
            left = i;
        } else {
            if (i < right) sort_edges_rec(edges, i, right, depth_limit);
            right = j;
        }
    }
}

inline void sort_edges(Edge* edges, int64_t left, int64_t right) noexcept {
    if (left >= right) return;
    int64_t n = right - left + 1;
    int64_t depth = 0;
    while (n > 1) {
        depth += 2;
        n >>= 1;
    }
    sort_edges_rec(edges, left, right, depth);
}

// ============================================================================
// 3. Search & Disjoint Set Union (DSU)
// ============================================================================

inline int64_t lower_bound_i64(const int64_t* arr, int64_t n, int64_t target) noexcept {
    int64_t low = 0;
    int64_t high = n;
    while (low < high) {
        int64_t mid = low + (high - low) / 2;
        if (arr[mid] < target) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    return low;
}

inline int64_t dsu_find(int64_t* parent, int64_t i) noexcept {
    int64_t root = i;
    while (parent[root] != root) {
        root = parent[root];
    }
    int64_t curr = i;
    while (curr != root) {
        int64_t nxt = parent[curr];
        parent[curr] = root;
        curr = nxt;
    }
    return root;
}

inline bool dsu_union(int64_t* parent, int64_t* rank, int64_t x, int64_t y) noexcept {
    int64_t rx = dsu_find(parent, x);
    int64_t ry = dsu_find(parent, y);
    if (rx == ry) return false;
    if (rank[rx] < rank[ry]) {
        parent[rx] = ry;
    } else if (rank[rx] > rank[ry]) {
        parent[ry] = rx;
    } else {
        parent[ry] = rx;
        rank[rx] += 1;
    }
    return true;
}

// ============================================================================
// 4. Core Manhattan MST Solver
// ============================================================================

int64_t solve_manhattan_mst_core(
    const int64_t* orig_x, const int64_t* orig_y, int64_t n, bool use_pure_heap
) noexcept {
    if (n <= 1) return 0;
    if (!validate_coordinates(orig_x, orig_y, n)) {
        std::printf("[ERROR] Coordinates out of supported range [%lld, %lld]\n", COORD_MIN, COORD_MAX);
        return -1;
    }

    // Modern C++23 RAII: vectors automatically freed on scope exit
    std::vector<Point> pts(n);
    std::vector<int64_t> z_vals(n);
    std::vector<int64_t> bit_val(n + 4);
    std::vector<int64_t> bit_id(n + 4);
    int64_t max_edges = 4 * n + 10;
    std::vector<Edge> edges(max_edges);
    int64_t edge_count = 0;

    for (int64_t i = 0; i < n; ++i) {
        pts[i] = Point{orig_x[i], orig_y[i], i};
    }

    constexpr int64_t inf = INF_DIST;

    for (int dir = 0; dir < 4; ++dir) {
        if (dir == 1 || dir == 3) {
            for (int64_t i = 0; i < n; ++i) {
                std::swap(pts[i].x, pts[i].y);
            }
        } else if (dir == 2) {
            for (int64_t i = 0; i < n; ++i) {
                pts[i].x = -pts[i].x;
            }
        }

        // 1. Sort points descending by X, then descending by Y
        if (use_pure_heap) {
            heapsort_points(pts.data(), 0, n - 1);
        } else {
            sort_points(pts.data(), 0, n - 1);
        }

        // 2. Coordinate compression of (Y - X)
        for (int64_t i = 0; i < n; ++i) {
            z_vals[i] = pts[i].y - pts[i].x;
        }
        if (use_pure_heap) {
            heapsort_i64(z_vals.data(), 0, n - 1);
        } else {
            sort_i64(z_vals.data(), 0, n - 1);
        }

        int64_t m = 0;
        if (n > 0) {
            m = 1;
            for (int64_t i = 1; i < n; ++i) {
                if (z_vals[i] != z_vals[m - 1]) {
                    z_vals[m] = z_vals[i];
                    ++m;
                }
            }
        }

        // 3. Initialize Fenwick tree
        for (int64_t p = 0; p <= m + 2; ++p) {
            bit_val[p] = inf;
            bit_id[p] = -1;
        }

        // 4. Sweep line
        for (int64_t i = 0; i < n; ++i) {
            int64_t z = pts[i].y - pts[i].x;
            int64_t rank = lower_bound_i64(z_vals.data(), m, z);
            int64_t pos = m - rank; // 1-based index in [1, m]

            // Query prefix [1, pos]
            int64_t best_id = -1;
            int64_t min_val = inf;
            int64_t p = pos;
            while (p > 0) {
                if (bit_val[p] < min_val) {
                    min_val = bit_val[p];
                    best_id = bit_id[p];
                }
                int64_t lowbit = p & -p;
                p -= lowbit;
            }

            if (best_id != -1) {
                int64_t u = pts[i].id;
                int64_t v = best_id;
                int64_t dist = abs_i64(orig_x[u] - orig_x[v]) + abs_i64(orig_y[u] - orig_y[v]);
                edges[edge_count] = Edge{u, v, dist};
                ++edge_count;
            }

            // Update Fenwick tree with pts[i]
            int64_t val = pts[i].x + pts[i].y;
            int64_t id = pts[i].id;
            int64_t up = pos;
            while (up <= m) {
                if (val < bit_val[up]) {
                    bit_val[up] = val;
                    bit_id[up] = id;
                }
                int64_t lowbit = up & -up;
                up += lowbit;
            }
        }
    }

    // 5. Kruskal's algorithm on candidate edges
    if (edge_count > 0) {
        if (use_pure_heap) {
            heapsort_edges(edges.data(), 0, edge_count - 1);
        } else {
            sort_edges(edges.data(), 0, edge_count - 1);
        }
    }

    std::vector<int64_t> parent(n);
    std::vector<int64_t> rank(n, 0);
    std::iota(parent.begin(), parent.end(), 0LL);

    int64_t total_mst_weight = 0;
    int64_t edges_added = 0;
    for (int64_t i = 0; i < edge_count; ++i) {
        const auto& e = edges[i];
        if (dsu_union(parent.data(), rank.data(), e.u, e.v)) {
            total_mst_weight += e.w;
            ++edges_added;
            if (edges_added == n - 1) break;
        }
    }

    return total_mst_weight;
}

int64_t solve_manhattan_mst(const int64_t* orig_x, const int64_t* orig_y, int64_t n) noexcept {
    return solve_manhattan_mst_core(orig_x, orig_y, n, false);
}

int64_t solve_manhattan_mst_pure_heapsort(const int64_t* orig_x, const int64_t* orig_y, int64_t n) noexcept {
    return solve_manhattan_mst_core(orig_x, orig_y, n, true);
}

// O(N^2) Prim's algorithm reference implementation for verification
int64_t solve_manhattan_mst_bruteforce(const int64_t* orig_x, const int64_t* orig_y, int64_t n) noexcept {
    if (n <= 1) return 0;
    std::vector<int64_t> min_dist(n, INF_DIST);
    std::vector<bool> visited(n, false);
    min_dist[0] = 0;

    int64_t total_weight = 0;
    for (int64_t step = 0; step < n; ++step) {
        int64_t u = -1;
        int64_t best_d = INF_DIST;
        for (int64_t i = 0; i < n; ++i) {
            if (!visited[i] && min_dist[i] < best_d) {
                best_d = min_dist[i];
                u = i;
            }
        }

        visited[u] = true;
        total_weight += best_d;

        for (int64_t v = 0; v < n; ++v) {
            if (!visited[v]) {
                int64_t d = abs_i64(orig_x[u] - orig_x[v]) + abs_i64(orig_y[u] - orig_y[v]);
                if (d < min_dist[v]) {
                    min_dist[v] = d;
                }
            }
        }
    }

    return total_weight;
}

// ============================================================================
// 5. Test Suite (Matches all 15 Goraw shadow-tests)
// ============================================================================

void run_all_tests() noexcept {
    std::printf("[C++23] Running test suite (15 tests)...\n");

    // 1. abs_i64
    assert(abs_i64(10) == 10);
    assert(abs_i64(-42) == 42);
    assert(abs_i64(0) == 0);
    assert(abs_i64(-9223372036854775807LL - 1LL) == 9223372036854775807LL);

    // 2. validate_coordinates
    int64_t vx[2] = {500, COORD_MAX};
    int64_t vy[2] = {-500, COORD_MIN};
    assert(validate_coordinates(vx, vy, 2) == true);
    vx[1] = COORD_MAX + 1;
    assert(validate_coordinates(vx, vy, 2) == false);

    // 3. point_greater
    assert(point_greater(Point{5, 2, 0}, Point{3, 10, 1}) == true);
    assert(point_greater(Point{3, 10, 0}, Point{5, 2, 1}) == false);
    assert(point_greater(Point{4, 7, 0}, Point{4, 3, 1}) == true);
    assert(point_greater(Point{4, 3, 0}, Point{4, 7, 1}) == false);

    // 4. sample_3_points
    int64_t x3[3] = {0, 1, 2};
    int64_t y3[3] = {0, 2, 1};
    assert(solve_manhattan_mst(x3, y3, 3) == 5);

    // 5. sample_square
    int64_t x4[4] = {0, 0, 10, 10};
    int64_t y4[4] = {0, 10, 0, 10};
    assert(solve_manhattan_mst(x4, y4, 4) == 30);

    // 6. collinear horizontal
    int64_t ch_x[5] = {10, 2, 8, 0, 4};
    int64_t ch_y[5] = {5, 5, 5, 5, 5};
    assert(solve_manhattan_mst(ch_x, ch_y, 5) == 10);

    // 7. collinear vertical
    int64_t cv_x[5] = {-3, -3, -3, -3, -3};
    int64_t cv_y[5] = {20, -10, 0, 5, 12};
    assert(solve_manhattan_mst(cv_x, cv_y, 5) == 30);

    // 8. collinear diagonal pos
    int64_t cdp_x[4] = {0, 3, 1, 7};
    int64_t cdp_y[4] = {0, 3, 1, 7};
    assert(solve_manhattan_mst(cdp_x, cdp_y, 4) == 14);

    // 9. collinear diagonal neg
    int64_t cdn_x[4] = {0, 3, 1, 7};
    int64_t cdn_y[4] = {0, -3, -1, -7};
    assert(solve_manhattan_mst(cdn_x, cdn_y, 4) == 14);

    // 10. grid 4x4
    int64_t gx[16], gy[16];
    int64_t idx = 0;
    for (int64_t r = 0; r < 4; ++r) {
        for (int64_t c = 0; c < 4; ++c) {
            gx[idx] = c * 10;
            gy[idx] = r * 10;
            ++idx;
        }
    }
    assert(solve_manhattan_mst(gx, gy, 16) == 150);

    // 11. stress random 100 vs Prim
    uint64_t state = 123456789ULL;
    auto next_rand = [&]() -> uint64_t {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        return state;
    };
    int64_t rx[100], ry[100];
    for (int i = 0; i < 100; ++i) {
        rx[i] = (static_cast<int64_t>(next_rand() % 20001)) - 10000;
        ry[i] = (static_cast<int64_t>(next_rand() % 20001)) - 10000;
    }
    int64_t fast_mst = solve_manhattan_mst(rx, ry, 100);
    int64_t heap_mst = solve_manhattan_mst_pure_heapsort(rx, ry, 100);
    int64_t prim_mst = solve_manhattan_mst_bruteforce(rx, ry, 100);
    assert(fast_mst == prim_mst);
    assert(heap_mst == prim_mst);

    // 12. stress collinear random
    for (int i = 0; i < 100; ++i) {
        rx[i] = static_cast<int64_t>(next_rand() % 50001);
        ry[i] = 12345;
    }
    assert(solve_manhattan_mst(rx, ry, 100) == solve_manhattan_mst_bruteforce(rx, ry, 100));

    // 13. Adversarial patterns (strictly checking all N elements)
    int64_t n_adv = 10000;
    std::vector<int64_t> adv(n_adv);
    for (int64_t i = 0; i < n_adv; ++i) adv[i] = n_adv - 1 - i;
    sort_i64(adv.data(), 0, n_adv - 1);
    for (int64_t i = 0; i < n_adv - 1; ++i) assert(adv[i] <= adv[i + 1]);

    // Constant array
    std::fill(adv.begin(), adv.end(), 42LL);
    sort_i64(adv.data(), 0, n_adv - 1);
    for (int64_t i = 0; i < n_adv; ++i) assert(adv[i] == 42);

    // Sawtooth
    for (int64_t i = 0; i < n_adv; ++i) adv[i] = i % 100;
    sort_i64(adv.data(), 0, n_adv - 1);
    for (int64_t i = 0; i < n_adv - 1; ++i) assert(adv[i] <= adv[i + 1]);

    // 14. heapsort points direct verification
    std::vector<Point> hp_pts(100);
    for (int i = 0; i < 100; ++i) {
        hp_pts[i] = Point{
            static_cast<int64_t>(next_rand() % 1000),
            static_cast<int64_t>(next_rand() % 1000),
            i
        };
    }
    heapsort_points(hp_pts.data(), 0, 99);
    for (int i = 0; i < 99; ++i) {
        assert(!point_greater(hp_pts[i + 1], hp_pts[i]));
    }

    // 15. forced pure heapsort mst
    assert(solve_manhattan_mst_pure_heapsort(x3, y3, 3) == 5);
    assert(solve_manhattan_mst_pure_heapsort(x4, y4, 4) == 30);

    std::printf("[C++23] All 15 tests PASSED successfully!\n");
}

void benchmark_100k() noexcept {
    int64_t n = 100000;
    std::printf("Running C++23 benchmark on N = %lld points (Hard constraint)...\n", n);

    std::vector<int64_t> x(n);
    std::vector<int64_t> y(n);
    uint64_t state = 88172645463325252ULL;
    for (int64_t i = 0; i < n; ++i) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        x[i] = (static_cast<int64_t>(state % 2000000001ULL)) - 1000000000LL;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        y[i] = (static_cast<int64_t>(state % 2000000001ULL)) - 1000000000LL;
    }

    auto t0 = std::chrono::high_resolution_clock::now();
    int64_t mst = solve_manhattan_mst(x.data(), y.data(), n);
    auto t1 = std::chrono::high_resolution_clock::now();
    double ms = std::chrono::duration<double, std::milli>(t1 - t0).count();

    std::printf("100,000 points MST computed successfully: Total weight = %lld in %.2f ms\n", mst, ms);
}

int main() {
    std::printf("============================================================\n");
    std::printf("  Manhattan MST - O(N log N) Algorithm in C++23            \n");
    std::printf("============================================================\n\n");

    int64_t x3[3] = {0, 1, 2};
    int64_t y3[3] = {0, 2, 1};
    int64_t mst3 = solve_manhattan_mst(x3, y3, 3);
    std::printf("Sample 3 points: MST = %lld (Expected 5)\n", mst3);

    int64_t x4[4] = {0, 0, 10, 10};
    int64_t y4[4] = {0, 10, 0, 10};
    int64_t mst4 = solve_manhattan_mst(x4, y4, 4);
    std::printf("Sample square 4 points: MST = %lld (Expected 30)\n", mst4);

    run_all_tests();
    std::printf("\n");
    benchmark_100k();

    std::printf("\nDone! C++23 Manhattan MST completed successfully.\n");
    return 0;
}
