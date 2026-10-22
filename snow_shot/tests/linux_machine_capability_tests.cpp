#include "snow_shot/app/shellimagelaunch.h"
#include "snow_shot/platform/windows/autostartregistration.h"
#include "snow_shot/platform/windows/scrollinput.h"
#include "globalshortcutbackend_p.h"

#include <QGuiApplication>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusReply>
#include <QCursor>
#include <QStringList>

#include <cstdlib>
#include <iostream>

namespace {
void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(1);
    }
}

bool busName(const QString& name) {
    const auto listed = [](const char* method, const QString& name) {
        const QDBusMessage message = QDBusMessage::createMethodCall(
            QStringLiteral("org.freedesktop.DBus"), QStringLiteral("/org/freedesktop/DBus"),
            QStringLiteral("org.freedesktop.DBus"), QString::fromLatin1(method));
        const QDBusReply<QStringList> reply = QDBusConnection::sessionBus().call(message);
        return reply.isValid() && reply.value().contains(name);
    };
    return listed("ListNames", name) || listed("ListActivatableNames", name);
}
} // namespace

int main(int argc, char** argv) {
    QGuiApplication application(argc, argv);
    using snow_shot::presentation::createLinuxGlobalShortcutBackend;
    using snow_shot::shortcuts::ShortcutBinding;

    auto shortcuts = createLinuxGlobalShortcutBackend();
    const auto chord = shortcuts->validateShortcut(ShortcutBinding(QStringLiteral("Ctrl+Alt+A")));
    require(chord.supported && chord.shortcut == QStringLiteral("Ctrl+Alt+A"),
            "Ctrl+Alt+A must be a usable Linux shortcut");
    require(!shortcuts->validateShortcut(ShortcutBinding(QStringLiteral("Shift"))).supported,
            "a bare modifier is not a shortcut");
    require(!shortcuts->validateShortcut(ShortcutBinding{}).supported,
            "an empty shortcut is not usable");
    const auto registered = shortcuts->registerShortcut(7, ShortcutBinding(QStringLiteral("Ctrl+Alt+A")));
    require(registered.registered, "Linux must register a shortcut through the portal or X11");
    shortcuts->unregisterShortcut(7);

    using snow_shot::platform::windows::sendScrollingWheelStep;
    require(sendScrollingWheelStep({}, QPoint(0, -120)).status ==
                snow_shot::platform::windows::ScrollInputResult::Status::InvalidRequest,
            "an empty selection cannot scroll");
    const QPoint cursor = QCursor::pos();
    const auto posted = sendScrollingWheelStep(QRect(cursor, QSize(20, 20)), QPoint(0, -120));
    require(posted.status == snow_shot::platform::windows::ScrollInputResult::Status::Posted,
            "one vertical wheel notch must be delivered on this desktop");

    require(busName(QStringLiteral("org.freedesktop.portal.Desktop")),
            "the desktop portal must be available for capture and shortcuts");
    require(busName(QStringLiteral("org.gnome.Mutter.ScreenCast")),
            "Mutter screen cast must be available for capture");
    require(busName(QStringLiteral("org.freedesktop.FileManager1")),
            "the file manager must be able to reveal an item");

    require(snow_shot::platform::windows::AutoStartRegistration::isSupported(),
            "XDG autostart must be writable");
    const auto image = snow_shot::app::parseShellImageLaunchRequest(
        {QStringLiteral("/tmp/snow-shot-capability.png")});
    require(image.requested && image.valid &&
                image.paths == QStringList{QStringLiteral("/tmp/snow-shot-capability.png")},
            "a desktop file image path must launch the editor");
    const auto edited = snow_shot::app::parseShellImageLaunchRequest(
        {QStringLiteral("--edit-with-snow-shot"), QStringLiteral("/tmp/snow-shot-capability.png")});
    require(edited.requested && edited.valid, "the desktop edit action must accept an image path");
    return 0;
}
