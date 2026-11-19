/* Included before HashSet.cpp for the native analogue. The Emscripten build's C++ library
 * provides these headers transitively; GCC's libstdc++ does not, so they are named here. */
#include <algorithm>
#include <cstring>
#include <limits>
#include <memory>
#include <new>
#include <type_traits>
#include <utility>
