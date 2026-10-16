#ifndef SNOW_SHOT_PLATFORM_MACOS_APPLICATIONACTIVATION_H
#define SNOW_SHOT_PLATFORM_MACOS_APPLICATIONACTIVATION_H

#include <functional>
#include <memory>
#include <QtGlobal>

class QWidget;

namespace snow_shot::platform::macos {
void activateWindow(QWidget* window);
void configureMainWindowTitleBar(QWidget* window, int titleBarHeight);
qint64 foregroundApplicationProcessId();
// Return keyboard focus only while this app still owns it, never after the user switches away.
bool restoreForegroundApplication(qint64 processId);

class ApplicationReopenHandler final {
  public:
    explicit ApplicationReopenHandler(std::function<void()> reopen);
    ~ApplicationReopenHandler();

    ApplicationReopenHandler(const ApplicationReopenHandler&) = delete;
    ApplicationReopenHandler& operator=(const ApplicationReopenHandler&) = delete;

  private:
    class Impl;
    std::unique_ptr<Impl> m_impl;
};
} // namespace snow_shot::platform::macos

#endif // SNOW_SHOT_PLATFORM_MACOS_APPLICATIONACTIVATION_H
