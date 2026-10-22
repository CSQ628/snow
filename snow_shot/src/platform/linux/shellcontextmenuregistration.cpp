#include "snow_shot/platform/windows/shellcontextmenuregistration.h"

#include <QCoreApplication>

namespace snow_shot::platform::windows {

// Linux desktop environments do not expose a shell context-menu API that
// matches the Windows registry model. File associations are handled through
// the installed .desktop file, so these stubs report the feature as
// unavailable and keep the settings backend compiling.

ShellContextMenuRegistration::ShellContextMenuRegistration() = default;

ShellContextMenuRegistration::ShellContextMenuRegistration(Options options)
    : m_associationsRoot(std::move(options.classesRoot)),
      m_executable(std::move(options.executablePath)),
      m_productId(std::move(options.productId)),
      m_verbName(std::move(options.verbName)),
      m_extensions(std::move(options.fileExtensions)),
      m_registry(std::move(options.registry)),
      m_packagedPreferencePath(std::move(options.packagedPreferencePath)) {}

bool ShellContextMenuRegistration::isSupported() {
    return false;
}

QString ShellContextMenuRegistration::expectedCommand() const {
    return QString();
}

ShellContextMenuRegistrationSnapshot ShellContextMenuRegistration::snapshot() const {
    ShellContextMenuRegistrationSnapshot snapshot;
    snapshot.valid = false;
    snapshot.error = QCoreApplication::translate(
        "ShellContextMenuRegistration",
        "Shell context menus are not supported on Linux.");
    return snapshot;
}

bool ShellContextMenuRegistration::restore(const ShellContextMenuRegistrationSnapshot& previous,
                                           QString* error) const {
    Q_UNUSED(previous);
    if (error)
        *error = QCoreApplication::translate(
            "ShellContextMenuRegistration",
            "Shell context menus are not supported on Linux.");
    return false;
}

bool ShellContextMenuRegistration::setEnabled(bool enabled, QString* error) const {
    Q_UNUSED(enabled);
    if (error)
        *error = QCoreApplication::translate(
            "ShellContextMenuRegistration",
            "Shell context menus are not supported on Linux.");
    return false;
}

bool ShellContextMenuRegistration::setEnabled(bool enabled, const QString& accessKey,
                                              QString* error) const {
    Q_UNUSED(accessKey);
    return setEnabled(enabled, error);
}

bool ShellContextMenuRegistration::refreshLabels(QString* error) const {
    Q_UNUSED(error);
    return true;
}

bool ShellContextMenuRegistration::refreshLabels(const QString& accessKey, QString* error) const {
    Q_UNUSED(accessKey);
    return refreshLabels(error);
}

ShellContextMenuResult ShellContextMenuRegistration::changeEnabled(
    bool enabled, const std::function<bool()>& commitPreference) const {
    Q_UNUSED(enabled);
    Q_UNUSED(commitPreference);
    ShellContextMenuResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "ShellContextMenuRegistration",
        "Shell context menus are not supported on Linux.");
    return result;
}

ShellContextMenuResult ShellContextMenuRegistration::changeEnabled(
    bool enabled, const QString& accessKey,
    const std::function<bool()>& commitPreference) const {
    Q_UNUSED(accessKey);
    return changeEnabled(enabled, commitPreference);
}

ShellContextMenuResult ShellContextMenuRegistration::changePackagedEnabled(
    bool enabled, const QString& accessKey,
    const std::function<bool()>& commitPreference) const {
    Q_UNUSED(accessKey);
    return changeEnabled(enabled, commitPreference);
}

bool ShellContextMenuRegistration::ownedByProduct(const ShellContextMenuRegistryKey& key) const {
    Q_UNUSED(key);
    return false;
}

ShellContextMenuRegistryKey ShellContextMenuRegistration::enabledKey(
    ShellContextMenuRegistryKey previous, const QString& accessKey) const {
    Q_UNUSED(accessKey);
    return previous;
}

bool ShellContextMenuRegistration::apply(bool enabled, const QString& accessKey,
                                         const ShellContextMenuRegistrationSnapshot& previous,
                                         QString* error) const {
    Q_UNUSED(enabled);
    Q_UNUSED(accessKey);
    Q_UNUSED(previous);
    if (error)
        *error = QCoreApplication::translate(
            "ShellContextMenuRegistration",
            "Shell context menus are not supported on Linux.");
    return false;
}

} // namespace snow_shot::platform::windows
