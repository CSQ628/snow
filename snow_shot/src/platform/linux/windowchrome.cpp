#include "snow_shot/platform/windowcaptureexclusion.h"

#include <QWidget>

namespace snow_shot::platform {
bool setWindowExcludedFromCapture(QWidget* window, bool excluded) {
    if (window == nullptr)
        return false;
    window->setProperty("snowShotExcludedFromCapture", excluded);
    return true;
}

std::optional<std::uint32_t> captureWindowId(QWidget* window) {
    if (window == nullptr || window->winId() == 0)
        return std::nullopt;
    return static_cast<std::uint32_t>(window->winId() & 0xffffffffu);
}
} // namespace snow_shot::platform
