#include "snow_shot/platform/windows/administratorlaunch.h"

#include <QCoreApplication>

namespace snow_shot::platform::windows {

// Linux has no administrator elevation model matching Windows UAC. These
// stubs report that elevation is unsupported and keep the settings backend
// functional without a Linux-specific branch in every caller.

AdministratorPresentation administratorPresentation(AdministratorState state, bool autoStart,
                                                    bool pending) {
    Q_UNUSED(state);
    Q_UNUSED(autoStart);
    Q_UNUSED(pending);
    AdministratorPresentation presentation;
    presentation.launchEnabled = false;
    presentation.restartEnabled = false;
    presentation.elevated = false;
    presentation.launchHint = QCoreApplication::translate(
        "AdministratorLaunch", "Launching as administrator is not available on Linux.");
    presentation.restartHint = presentation.launchHint;
    presentation.restartLabel = QCoreApplication::translate("AdministratorLaunch",
                                                            "Restart as administrator");
    return presentation;
}

AdministratorResult runRestartTransaction(
    const std::function<AdministratorResult(const std::function<bool()>&)>& prepare,
    const std::function<bool()>& flush, const std::function<void()>& quit) {
    Q_UNUSED(prepare);
    Q_UNUSED(flush);
    Q_UNUSED(quit);
    AdministratorResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "AdministratorLaunch", "Administrator restart is not supported on Linux.");
    return result;
}

AdministratorResult runStartupTransaction(const StartupTransactionOperations& operations) {
    Q_UNUSED(operations);
    AdministratorResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "AdministratorLaunch", "Startup transactions are not supported on Linux.");
    return result;
}

AdministratorState administratorState() {
    AdministratorState state;
    state.valid = false;
    state.member = false;
    state.elevated = false;
    return state;
}

std::optional<StartupMode> observedStartupMode() {
    return std::nullopt;
}

QString canonicalAccountSid(const QString& accountOrSid) {
    return accountOrSid;
}

bool sameAccountSid(const QString& left, const QString& right) {
    return left == right;
}

bool administratorOperationPending() {
    return false;
}

void setAdministratorRestartGuard(std::function<bool()> guard) {
    Q_UNUSED(guard);
}

AdministratorResult changeStartupMode(StartupMode mode, const std::function<bool()>& persist) {
    Q_UNUSED(mode);
    Q_UNUSED(persist);
    AdministratorResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "AdministratorLaunch", "Changing startup mode is not supported on Linux.");
    return result;
}

AdministratorResult restartAsAdministrator(const std::function<bool()>& flush) {
    Q_UNUSED(flush);
    AdministratorResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "AdministratorLaunch", "Restarting as administrator is not supported on Linux.");
    return result;
}

int dispatchAdministratorHelper(const QStringList& arguments) {
    Q_UNUSED(arguments);
    return -1;
}

AdministratorResult reconcileStartupMode(StartupMode mode) {
    Q_UNUSED(mode);
    AdministratorResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "AdministratorLaunch", "Startup reconciliation is not supported on Linux.");
    return result;
}

void reconcileStartupRunValue(const QString& usersRunKey, const QString& expectedCommand,
                              const QString& replacementCommand) {
    Q_UNUSED(usersRunKey);
    Q_UNUSED(expectedCommand);
    Q_UNUSED(replacementCommand);
}

AdministratorResult removeInstallationStartup(const QString& root) {
    Q_UNUSED(root);
    AdministratorResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "AdministratorLaunch", "Startup removal is not supported on Linux.");
    return result;
}

AdministratorResult migrateInstallationStartup(const QString& previousRoot, const QString& root) {
    Q_UNUSED(previousRoot);
    Q_UNUSED(root);
    AdministratorResult result;
    result.success = false;
    result.error = QCoreApplication::translate(
        "AdministratorLaunch", "Startup migration is not supported on Linux.");
    return result;
}

bool launchOnInteractiveDesktop(const QString& executable) {
    Q_UNUSED(executable);
    return false;
}

} // namespace snow_shot::platform::windows
