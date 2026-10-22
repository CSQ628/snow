#include "snow_shot/platform/windows/autostartregistration.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>
#include <QTextStream>

namespace snow_shot::platform::windows {
namespace {

QString autostartDirectory() {
    QString config = QStandardPaths::writableLocation(QStandardPaths::ConfigLocation);
    if (config.isEmpty())
        config = QDir::homePath() + QStringLiteral("/.config");
    return config + QStringLiteral("/autostart");
}

QString desktopFilePath() {
    const QString appName = QCoreApplication::applicationName().isEmpty()
                                ? QStringLiteral("snow-shot")
                                : QCoreApplication::applicationName();
    return autostartDirectory() + QStringLiteral("/") + appName + QStringLiteral(".desktop");
}

QString desktopEntry(const QString& executable) {
    QString entry;
    QTextStream stream(&entry);
    stream << "[Desktop Entry]\n"
           << "Type=Application\n"
           << "Name=Snow Shot\n"
           << "Exec=" << executable << "\n"
           << "Terminal=false\n"
           << "X-GNOME-Autostart-enabled=true\n";
    return entry;
}

} // namespace

bool AutoStartRegistration::isSupported() {
    return !QStandardPaths::writableLocation(QStandardPaths::ConfigLocation).isEmpty();
}

QString AutoStartRegistration::hint() {
    return QCoreApplication::translate("AutoStartRegistration",
                                       "Start automatically when you log in.");
}

QString AutoStartRegistration::expectedCommand() {
    return QCoreApplication::applicationFilePath();
}

AutoStartRegistrationSnapshot AutoStartRegistration::snapshot() {
    AutoStartRegistrationSnapshot snapshot;
    const QString path = desktopFilePath();
    QFileInfo info(path);
    snapshot.valid = true;
    snapshot.exists = info.exists();
    snapshot.state = snapshot.exists ? AutoStartState::Enabled : AutoStartState::Disabled;
    if (snapshot.exists) {
        QFile file(path);
        if (file.open(QIODevice::ReadOnly | QIODevice::Text))
            snapshot.nativeData = file.readAll();
    }
    return snapshot;
}

bool AutoStartRegistration::matchesExpectedCommand() {
    const AutoStartRegistrationSnapshot current = snapshot();
    if (!current.exists)
        return false;
    return current.nativeData.contains(expectedCommand().toUtf8());
}

bool AutoStartRegistration::setEnabled(bool enabled, QString* error) {
    const QString path = desktopFilePath();
    if (enabled) {
        QDir dir(autostartDirectory());
        if (!dir.exists() && !dir.mkpath(QStringLiteral("."))) {
            if (error)
                *error = QCoreApplication::translate("AutoStartRegistration",
                                                     "Could not create the autostart directory.");
            return false;
        }
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Text | QIODevice::Truncate)) {
            if (error)
                *error = file.errorString();
            return false;
        }
        file.write(desktopEntry(expectedCommand()).toUtf8());
        return true;
    }
    if (QFile::exists(path) && !QFile::remove(path)) {
        if (error)
            *error = QCoreApplication::translate("AutoStartRegistration",
                                                 "Could not remove the autostart entry.");
        return false;
    }
    return true;
}

bool AutoStartRegistration::restore(const AutoStartRegistrationSnapshot& snapshot, QString* error) {
    if (!snapshot.valid) {
        if (error)
            *error = QCoreApplication::translate("AutoStartRegistration",
                                                 "The saved autostart state is invalid.");
        return false;
    }
    return setEnabled(snapshot.exists, error);
}

} // namespace snow_shot::platform::windows
