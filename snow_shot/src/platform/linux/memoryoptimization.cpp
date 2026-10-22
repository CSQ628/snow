#include "snow_shot/runtime/memoryoptimizationcontroller.h"

namespace snow_shot::runtime {
MemoryOptimizationController::Options nativeMemoryOptimizationOptions() {
    return {};
}

bool currentProcessOwnsForegroundWindow() {
    return false;
}
} // namespace snow_shot::runtime
