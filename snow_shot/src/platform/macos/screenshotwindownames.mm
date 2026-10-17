// SPDX-License-Identifier: GPL-3.0-only
#include "snow_shot/platform/screenshotwindownames.h"
#include "selectorwindowtarget_p.h"

namespace snow_shot::platform {
storage::ScreenshotWindowNames
screenshotWindowNamesAtPoint(const QPointF& point,
                             std::span<const std::uintptr_t> excludedWindows) {
    if (QGuiApplication::platformName() != QStringLiteral("cocoa"))
        return {};
    const CGWindowID hit = detail::selectorWindowAtDesktopPoint(point, excludedWindows);
    if (!hit)
        return {};
    CFArrayRef windows = CGWindowListCopyWindowInfo(kCGWindowListOptionIncludingWindow, hit);
    const auto target = detail::recaptureWindowTarget(windows, hit);
    storage::ScreenshotWindowNames result;
    if (target.id) {
        NSRunningApplication* application =
            [NSRunningApplication runningApplicationWithProcessIdentifier:target.pid];
        result.applicationName = QString::fromNSString(application.localizedName);
        if (result.applicationName.trimmed().isEmpty())
            result.applicationName = QString::fromNSString(
                application.executableURL.lastPathComponent.stringByDeletingPathExtension);
        for (CFIndex i = 0; i < CFArrayGetCount(windows); ++i) {
            const auto window = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(windows, i));
            CGWindowID id = 0;
            const auto number =
                static_cast<CFNumberRef>(CFDictionaryGetValue(window, kCGWindowNumber));
            if (!number || !CFNumberGetValue(number, kCFNumberIntType, &id) || id != hit)
                continue;
            const auto title = CFDictionaryGetValue(window, kCGWindowName);
            if (title && CFGetTypeID(title) == CFStringGetTypeID())
                result.windowName = QString::fromCFString(static_cast<CFStringRef>(title));
            break;
        }
    }
    if (windows)
        CFRelease(windows);
    return result;
}
} // namespace snow_shot::platform
