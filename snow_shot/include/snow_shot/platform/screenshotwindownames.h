// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "snow_shot/storage/screenshotexportvariables.h"
#include <QPointF>
#include <cstdint>
#include <span>

namespace snow_shot::platform {
// GUI thread. Native desktop coordinates: physical pixels on Windows, Quartz points on macOS.
[[nodiscard]] storage::ScreenshotWindowNames
screenshotWindowNamesAtPoint(const QPointF& point, std::span<const std::uintptr_t> excludedWindows);
} // namespace snow_shot::platform
