#include "snow_shot/platform/screenshotwindownames.h"

namespace snow_shot::platform {
storage::ScreenshotWindowNames
screenshotWindowNamesAtPoint(const QPointF& point, std::span<const std::uintptr_t> excludedWindows,
                             storage::ScreenshotWindowNameFields fields) {
    Q_UNUSED(point);
    Q_UNUSED(excludedWindows);
    Q_UNUSED(fields);
    return {};
}
} // namespace snow_shot::platform
