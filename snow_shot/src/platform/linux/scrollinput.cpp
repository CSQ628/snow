#include "snow_shot/platform/windows/scrollinput.h"

#include <QCursor>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QDBusReply>

#include <X11/Xlib.h>
#include <X11/extensions/XTest.h>

#ifdef Status
#undef Status
#endif

namespace {
int scrollSteps(int delta) {
    if (delta == 0)
        return 0;
    const int steps = delta / 120;
    if (steps != 0)
        return steps;
    return delta > 0 ? 1 : -1;
}

bool mutterAxis(QDBusInterface& session, unsigned axis, int steps) {
    if (steps == 0)
        return true;
    QDBusMessage message = QDBusMessage::createMethodCall(
        QStringLiteral("org.gnome.Mutter.RemoteDesktop"), session.path(),
        QStringLiteral("org.gnome.Mutter.RemoteDesktop.Session"),
        QStringLiteral("NotifyPointerAxisDiscrete"));
    message << quint32(0) << quint32(axis) << qint32(steps);
    return QDBusConnection::sessionBus().call(message).type() != QDBusMessage::ErrorMessage;
}

bool mutterScroll(const QPoint& center, const QPoint& delta, const QPoint& restore) {
    QDBusInterface desktop(QStringLiteral("org.gnome.Mutter.RemoteDesktop"),
                           QStringLiteral("/org/gnome/Mutter/RemoteDesktop"),
                           QStringLiteral("org.gnome.Mutter.RemoteDesktop"),
                           QDBusConnection::sessionBus());
    if (!desktop.isValid())
        return false;
    const QDBusReply<QDBusObjectPath> created = desktop.call(QStringLiteral("CreateSession"));
    if (!created.isValid())
        return false;
    QDBusInterface session(QStringLiteral("org.gnome.Mutter.RemoteDesktop"), created.value().path(),
                           QStringLiteral("org.gnome.Mutter.RemoteDesktop.Session"),
                           QDBusConnection::sessionBus());
    if (!session.isValid() ||
        session.call(QStringLiteral("Start")).type() == QDBusMessage::ErrorMessage)
        return false;
    auto moveTo = [&session](const QPoint& point) {
        QDBusMessage message = QDBusMessage::createMethodCall(
            QStringLiteral("org.gnome.Mutter.RemoteDesktop"), session.path(),
            QStringLiteral("org.gnome.Mutter.RemoteDesktop.Session"),
            QStringLiteral("NotifyPointerMotionAbsolute"));
        message << quint32(0) << double(point.x()) << double(point.y());
        return QDBusConnection::sessionBus().call(message).type() != QDBusMessage::ErrorMessage;
    };
    const bool moved = moveTo(center);
    const bool vertical = mutterAxis(session, 0, scrollSteps(delta.y()));
    const bool horizontal = mutterAxis(session, 1, scrollSteps(delta.x()));
    moveTo(restore);
    session.call(QStringLiteral("Stop"));
    return moved && vertical && horizontal;
}

bool xtestScroll(const QPoint& center, const QPoint& delta, const QPoint& restore) {
    Display* display = XOpenDisplay(nullptr);
    if (display == nullptr)
        return false;
    auto button = [&](int button, int times) {
        for (int index = 0; index < times; ++index) {
            XTestFakeButtonEvent(display, static_cast<unsigned>(button), True, CurrentTime);
            XTestFakeButtonEvent(display, static_cast<unsigned>(button), False, CurrentTime);
        }
    };
    XTestFakeMotionEvent(display, -1, center.x(), center.y(), CurrentTime);
    const int vertical = scrollSteps(delta.y());
    const int horizontal = scrollSteps(delta.x());
    if (vertical > 0)
        button(4, vertical);
    else if (vertical < 0)
        button(5, -vertical);
    if (horizontal > 0)
        button(7, horizontal);
    else if (horizontal < 0)
        button(6, -horizontal);
    XTestFakeMotionEvent(display, -1, restore.x(), restore.y(), CurrentTime);
    XFlush(display);
    XCloseDisplay(display);
    return true;
}
} // namespace

namespace snow_shot::platform::windows {
ScrollInputResult sendScrollingWheelStep(const QRect& physicalSelection, const QPoint& wheelDelta) {
    if (physicalSelection.isEmpty() || wheelDelta.isNull())
        return {ScrollInputResult::Status::InvalidRequest, 0};
    const QPoint center = physicalSelection.center();
    const QPoint restore = QCursor::pos();
    if (qEnvironmentVariableIsSet("WAYLAND_DISPLAY") && mutterScroll(center, wheelDelta, restore))
        return {ScrollInputResult::Status::Posted, 0};
    if (xtestScroll(center, wheelDelta, restore))
        return {ScrollInputResult::Status::Posted, 0};
    return {ScrollInputResult::Status::Unsupported, 0};
}
} // namespace snow_shot::platform::windows
