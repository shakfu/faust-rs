#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <string>
#include <vector>

#include "garbageable.hh"
#include "node.hh"
#include "tree.hh"

// Minimal local definitions so this standalone benchmark links without compiler/global.cpp.
void Garbageable::cleanup() {}

void* Garbageable::operator new(size_t size)
{
    return ::operator new(size);
}

void* Garbageable::operator new[](size_t size)
{
    return ::operator new[](size);
}

void Garbageable::operator delete(void* ptr)
{
    ::operator delete(ptr);
}

void Garbageable::operator delete[](void* ptr)
{
    ::operator delete[](ptr);
}

void faustassertaux(bool cond, const std::string&, int)
{
    if (!cond) {
        std::abort();
    }
}

int CTree::calcTreeAperture(const Node&, const tvec& br)
{
    int max_aperture = 0;
    for (Tree child : br) {
        if (child != nullptr && child->aperture() > max_aperture) {
            max_aperture = child->aperture();
        }
    }
    return max_aperture;
}

static size_t parse_n(int argc, char** argv)
{
    if (argc < 2) {
        return 200000;
    }
    char* end = nullptr;
    unsigned long long value = std::strtoull(argv[1], &end, 10);
    if (end == argv[1] || *end != '\0') {
        return 200000;
    }
    return static_cast<size_t>(value);
}

int main(int argc, char** argv)
{
    using clock = std::chrono::steady_clock;

    const size_t n = parse_n(argc, argv);

    Symbol::init();
    CTree::init();

    std::vector<Tree> nodes;
    nodes.reserve(n);

    const Sym pair_sym = symbol("pair");

    const auto create_start = clock::now();
    for (size_t i = 0; i < n; ++i) {
        Tree a = tree(Node(static_cast<int>(i)));
        Tree b = tree(Node(static_cast<int>(i + 1)));
        Tree t = tree(Node(pair_sym), a, b);
        nodes.push_back(t);
    }
    const auto create_end = clock::now();

    volatile Tree lookup_sink = nullptr;
    const auto lookup_start = clock::now();
    for (size_t i = 0; i < n; ++i) {
        Tree a = tree(Node(static_cast<int>(i)));
        Tree b = tree(Node(static_cast<int>(i + 1)));
        Tree t = tree(Node(pair_sym), a, b);
        lookup_sink = t;
    }
    const auto lookup_end = clock::now();

    const Sym nil_sym = symbol("nil");
    const Sym cons_sym = symbol("cons");

    Tree list = tree(Node(nil_sym));

    const auto traversal_start = clock::now();
    for (Tree t : nodes) {
        list = tree(Node(cons_sym), t, list);
    }

    size_t count = 0;
    for (Tree cur = list; cur != nullptr;) {
        Sym node_sym = nullptr;
        if (!isSym(cur->node(), &node_sym)) {
            break;
        }
        if (node_sym == nil_sym && cur->arity() == 0) {
            break;
        }
        if (node_sym != cons_sym || cur->arity() != 2) {
            break;
        }
        ++count;
        cur = cur->branch(1);
    }
    const auto traversal_end = clock::now();

    volatile size_t traversal_sink = count;

    Tree hot_key = tree(Node(symbol("hot")));

    const auto prop_set_start = clock::now();
    for (size_t i = 0; i < nodes.size(); ++i) {
        nodes[i]->setProperty(hot_key, tree(Node(static_cast<int>(i))));
    }
    const auto prop_set_end = clock::now();

    size_t checksum = 0;
    const auto prop_get_start = clock::now();
    for (Tree t : nodes) {
        Tree value = t->getProperty(hot_key);
        if (value != nullptr) {
            int v = 0;
            if (isInt(value->node(), &v)) {
                checksum ^= static_cast<size_t>(v);
            }
        }
    }
    const auto prop_get_end = clock::now();

    volatile size_t prop_sink = checksum;
    (void)lookup_sink;
    (void)traversal_sink;
    (void)prop_sink;

    auto to_ms = [](const clock::time_point& start, const clock::time_point& end) {
        return std::chrono::duration<double, std::milli>(end - start).count();
    };

    std::cout << "TreeArena C++ micro-bench (n=" << n << ")\n";
    std::cout << std::fixed << std::setprecision(3);
    std::cout << "create_ms=" << to_ms(create_start, create_end) << "\n";
    std::cout << "lookup_ms=" << to_ms(lookup_start, lookup_end) << "\n";
    std::cout << "traversal_ms=" << to_ms(traversal_start, traversal_end) << "\n";
    std::cout << "property_set_ms=" << to_ms(prop_set_start, prop_set_end) << "\n";
    std::cout << "property_get_ms=" << to_ms(prop_get_start, prop_get_end) << "\n";

    return 0;
}
