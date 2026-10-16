#include "snow_shot/platform/macos/applicationactivation.h"
#include "snow_shot/presentation/screenshotocrcopyfeedback.h"

#import <AppKit/AppKit.h>

#include <QApplication>
#include <QClipboard>
#include <QCoreApplication>
#include <QElapsedTimer>
#include <QThread>
#include <QPushButton>
#include <QTimer>
#include <QWidget>

#include <cmath>
#include <cstdlib>
#include <iostream>

namespace {
void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(EXIT_FAILURE);
    }
}

void flushEvents() {
    QCoreApplication::processEvents();
    QCoreApplication::sendPostedEvents();
    QCoreApplication::processEvents();
}
} // namespace

int main(int argc, char** argv) {
    QApplication application(argc, argv);

    if (const qsizetype fixture =
            application.arguments().indexOf(QStringLiteral("--quick-copy-handoff-fixture"));
        fixture >= 0) {
        require(fixture + 1 < application.arguments().size(), "fixture requires a target process");
        const qint64 target = application.arguments().at(fixture + 1).toLongLong();
        require(target > 0 && target != NSProcessInfo.processInfo.processIdentifier,
                "fixture target must be an independent application");
        application.setQuitOnLastWindowClosed(false);
        QWidget capture;
        ScreenshotOcrCopyFeedback feedback;
        capture.setWindowTitle(QObject::tr("Quick OCR activation test"));
        capture.resize(340, 100);
        QPushButton finish(QObject::tr("Close capture and return to input"), &capture);
        finish.setGeometry(20, 30, 300, 40);
        QObject::connect(&finish, &QPushButton::clicked, &application, [&] {
            const bool owned = snow_shot::platform::macos::foregroundApplicationProcessId() ==
                               NSProcessInfo.processInfo.processIdentifier;
            capture.hide();
            const bool accepted = snow_shot::platform::macos::restoreForegroundApplication(target);
            std::cout << "handoff owned=" << owned << " accepted=" << accepted << std::endl;
            require(owned && accepted, "visible capture must yield activation to its origin");
            QTimer::singleShot(300, &application, [&] {
                require(snow_shot::platform::macos::foregroundApplicationProcessId() == target,
                        "capture exit must restore the independent application");
                application.clipboard()->setText(QStringLiteral("REA Quick OCR\n中文粘贴测试"),
                                                 QClipboard::Clipboard);
                feedback.showCopied(QObject::tr("Text copied"));
                QTimer::singleShot(100, &application, [&] {
                    require(feedback.isVisible(), "successful copy feedback must be visible");
                    require(snow_shot::platform::macos::foregroundApplicationProcessId() == target,
                            "background clipboard publication must not take focus back");
                    std::cout << "native handoff and background clipboard passed target=" << target
                              << std::endl;
                });
            });
        });
        capture.show();
        std::cout << "handoff fixture ready target=" << target << std::endl;
        return application.exec();
    }

    if (application.arguments().contains(QStringLiteral("--quick-copy-focus-only"))) {
        using namespace snow_shot::platform::macos;
        const qint64 before = foregroundApplicationProcessId();
        require(before > 0 && before != NSProcessInfo.processInfo.processIdentifier,
                "background feedback test must begin with another foreground application");
        require(before == NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier,
                "capture must remember the current foreground application");
        if (before > 0 && before != NSProcessInfo.processInfo.processIdentifier)
            require(!restoreForegroundApplication(before),
                    "return must not activate after the user switches to another application");
        require(!restoreForegroundApplication(0), "missing capture origin must not activate");
        require(!restoreForegroundApplication(NSProcessInfo.processInfo.processIdentifier),
                "capture from Snow Shot must not reopen or raise its main window");
        require(!restoreForegroundApplication(2147483647),
                "a terminated capture origin must not activate");
        require(foregroundApplicationProcessId() == before,
                "rejected returns must preserve the current application focus");
        ScreenshotOcrCopyFeedback feedback;
        feedback.showCopied(QObject::tr("Text copied"));
        flushEvents();
        NSView* feedbackView = reinterpret_cast<NSView*>(feedback.internalWinId());
        require(feedback.isVisible() && feedbackView.window.visible &&
                    !feedbackView.window.canBecomeKeyWindow &&
                    feedbackView.window.ignoresMouseEvents,
                "native success feedback must be visible, non-key and click-through");
        require(foregroundApplicationProcessId() == before,
                "showing copy success must preserve the foreground application");
        QEventLoop expiry;
        QTimer::singleShot(1800, &expiry, &QEventLoop::quit);
        expiry.exec();
        require(!feedback.isVisible() && foregroundApplicationProcessId() == before,
                "success feedback must expire without changing application focus");
        std::cout << "quick OCR foreground return guards passed\n";
        return EXIT_SUCCESS;
    }

    NSDictionary* bundleInfo =
        [NSDictionary dictionaryWithContentsOfFile:@SNOW_SHOT_MACOS_INFO_PLIST];
    NSArray<NSString*>* localizations = bundleInfo[@"CFBundleLocalizations"];
    require(localizations.count == 3, "the bundle must advertise all three catalog languages");
    const auto requireNativeLanguage = [&](NSString* preference, NSString* expected) {
        NSArray<NSString*>* selected = [NSBundle preferredLocalizationsFromArray:localizations
                                                                  forPreferences:@[ preference ]];
        require([selected.firstObject isEqualToString:expected],
                "AppKit must resolve the system language to the matching application language");
    };
    requireNativeLanguage(@"en-US", @"en");
    requireNativeLanguage(@"zh-CN", @"zh-Hans");
    requireNativeLanguage(@"zh-TW", @"zh-Hant");
    requireNativeLanguage(@"zh-HK", @"zh-Hant");
    requireNativeLanguage(@"de-DE", @"en");

    id<NSApplicationDelegate> originalDelegate = NSApp.delegate;
    bool reopened = false;
    {
        snow_shot::platform::macos::ApplicationReopenHandler handler([&]() { reopened = true; });
        id<NSApplicationDelegate> proxy = NSApp.delegate;
        require(proxy != originalDelegate, "the reopen handler must install a forwarding proxy");
        require(
            [proxy respondsToSelector:@selector(applicationShouldHandleReopen:hasVisibleWindows:)],
            "the forwarding proxy must handle Dock reopen callbacks");
        [proxy applicationShouldHandleReopen:NSApp hasVisibleWindows:NO];
        require(reopened, "a Dock reopen callback must reach the application handler");
    }
    require(NSApp.delegate == originalDelegate,
            "destroying the reopen handler must restore Qt's application delegate");

    QWidget window(nullptr, Qt::Window | Qt::ExpandedClientAreaHint | Qt::NoTitleBarBackgroundHint);
    window.setAttribute(Qt::WA_LayoutOnEntireRect);
    window.resize(320, 180);
    window.show();
    flushEvents();
    NSView* view = reinterpret_cast<NSView*>(window.internalWinId());
    NSWindow* nativeWindow = view.window;
    require(nativeWindow != nil, "a Cocoa widget must expose an NSWindow");
    require(nativeWindow.level == NSNormalWindowLevel,
            "the main window must use the normal macOS window level after showing");
    const NSWindowCollectionBehavior originalBehavior = nativeWindow.collectionBehavior;
    NSButton* originalZoomButton = [nativeWindow standardWindowButton:NSWindowZoomButton];
    const SEL originalZoomAction = originalZoomButton.action;
    id originalZoomTarget = originalZoomButton.target;
    constexpr int titleBarHeight = 32;
    nativeWindow.level = NSFloatingWindowLevel;
    snow_shot::platform::macos::configureMainWindowTitleBar(&window, titleBarHeight);
    flushEvents();
    require(nativeWindow.level == NSNormalWindowLevel,
            "custom title-bar layout must preserve the normal macOS window level");
    const NSWindowStyleMask expected =
        NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable |
        NSWindowStyleMaskResizable | NSWindowStyleMaskFullSizeContentView;
    require((nativeWindow.styleMask & expected) == expected,
            "the SnowShot title bar must retain resizable native macOS window behavior");
    require(nativeWindow.titleVisibility == NSWindowTitleHidden &&
                nativeWindow.titlebarAppearsTransparent,
            "the native title text and surface must yield to SnowShot's in-content title bar");

    NSButton* closeButton = [nativeWindow standardWindowButton:NSWindowCloseButton];
    NSButton* minimizeButton = [nativeWindow standardWindowButton:NSWindowMiniaturizeButton];
    NSButton* zoomButton = [nativeWindow standardWindowButton:NSWindowZoomButton];
    require((nativeWindow.collectionBehavior & NSWindowCollectionBehaviorFullScreenPrimary) != 0 &&
                nativeWindow.collectionBehavior == originalBehavior,
            "custom title-bar layout must preserve native fullscreen eligibility");
    require(zoomButton == originalZoomButton && zoomButton.action == originalZoomAction &&
                zoomButton.target == originalZoomTarget && zoomButton.enabled,
            "custom title-bar layout must retain AppKit's enabled green button and action");
    require(closeButton != nil && minimizeButton != nil && zoomButton != nil &&
                !closeButton.hidden && !minimizeButton.hidden && !zoomButton.hidden,
            "AppKit must continue to own all three native traffic-light buttons");
    const NSRect closeFrame = [closeButton.superview convertRect:closeButton.frame toView:nil];
    const NSRect minimizeFrame = [minimizeButton.superview convertRect:minimizeButton.frame
                                                                toView:nil];
    const NSRect zoomFrame = [zoomButton.superview convertRect:zoomButton.frame toView:nil];
    const NSRect contentFrame =
        [nativeWindow.contentView convertRect:nativeWindow.contentView.bounds toView:nil];
    NSView* frameView = nativeWindow.contentView.superview;
    const NSRect contentFrameInFrameView =
        [nativeWindow.contentView convertRect:nativeWindow.contentView.bounds toView:frameView];
    require(std::abs(NSMaxY(contentFrameInFrameView) - NSMaxY(frameView.bounds)) <= 1.0,
            "the Qt client area must reach the top of the native frame without a second title row");
    require(NSMidX(closeFrame) < NSMidX(minimizeFrame) && NSMidX(minimizeFrame) < NSMidX(zoomFrame),
            "native traffic lights must retain close/minimize/zoom order on the left");
    require(std::abs((NSMinX(closeFrame) - NSMinX(contentFrame)) - 14.0) <= 1.0,
            "native traffic lights must align with SnowShot's left title-bar inset");
    require(std::abs((NSMaxY(contentFrame) - NSMidY(closeFrame)) - titleBarHeight / 2.0) <= 1.0,
            "native traffic lights must be vertically centered in SnowShot's title bar");
    snow_shot::platform::macos::activateWindow(&window);
    flushEvents();
    require(nativeWindow.level == NSNormalWindowLevel,
            "activation must preserve the normal macOS window level");
    require(nativeWindow.visible, "activation must keep the native window ordered in front");
    require(nativeWindow.keyWindow, "activation must make the native window key");

    window.hide();
    window.show();
    snow_shot::platform::macos::configureMainWindowTitleBar(&window, titleBarHeight);
    snow_shot::platform::macos::activateWindow(&window);
    flushEvents();
    require(nativeWindow.level == NSNormalWindowLevel,
            "reopening the main window must retain the normal macOS window level");

    const auto waitForFullscreen = [&](bool fullscreen) {
        QElapsedTimer timeout;
        timeout.start();
        while (timeout.elapsed() < 5000) {
            flushEvents();
            const bool nativeFullscreen =
                (nativeWindow.styleMask & NSWindowStyleMaskFullScreen) != 0;
            if (window.isFullScreen() == fullscreen && nativeFullscreen == fullscreen) {
                return true;
            }
            QThread::msleep(10);
        }
        return false;
    };
    [zoomButton performClick:nil];
    require(waitForFullscreen(true), "the native green button must enter macOS fullscreen");
    snow_shot::platform::macos::configureMainWindowTitleBar(&window, titleBarHeight);
    [zoomButton performClick:nil];
    require(waitForFullscreen(false), "the native green button must leave macOS fullscreen");
    require(nativeWindow.level == NSNormalWindowLevel,
            "leaving fullscreen must retain the normal macOS window level");
    return 0;
}
