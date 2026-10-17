// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <QDateTime>
#include <QString>

namespace snow_shot::storage {
struct ScreenshotWindowNames {
    QString applicationName;
    QString windowName;
    friend bool operator==(const ScreenshotWindowNames&, const ScreenshotWindowNames&) = default;
};

[[nodiscard]] bool containsScreenshotWindowVariables(const QString& text);
[[nodiscard]] QString expandScreenshotExportVariables(const QString& text,
                                                      const QDateTime& timestamp,
                                                      const ScreenshotWindowNames& names = {});
} // namespace snow_shot::storage
