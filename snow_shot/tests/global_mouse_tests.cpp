#include "snow_shot/presentation/globalmousemanager.h"
#include "snow_shot/presentation/globalshortcutmanager.h"
#include "snow_shot/presentation/screenshotglobalmousedrag.h"
#include "snow_shot/storage/applicationstorage.h"

#include <QApplication>
#include <QJsonObject>
#include <QJsonArray>
#include <QTemporaryDir>
#include <QSemaphore>
#include <QTimer>
#include <QThread>
#include <array>
#include <atomic>
#include <cstdlib>
#include <iostream>
#include <thread>

#ifdef Q_OS_WIN
void globalMouseNativeActivationKeyTests();
void globalMouseNativePerformanceTests();
void globalMouseNativeStopFromWorkerThreadTests();
#endif

namespace {
using namespace snow_shot::presentation;
using Action = settings::SettingsGlobalMouseAction;
using InputKind = GlobalMouseInput::Kind;
using EventKind = GlobalMouseDragEvent::Kind;

void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(1);
    }
}

GlobalMouseConfiguration configured(Qt::MouseButton button = Qt::LeftButton) {
    return {{{Action::ScreenshotCopy, GlobalMouseModifier::Control, button}}, true};
}

void everyCombinationMatchesExactlyAndLatchesItsAction() {
    GlobalMouseGesture empty;
    require(!empty.handle({InputKind::Release, {}, Qt::NoButton}, {}).consumed,
            "an unrecognized release must never acquire button ownership");
#ifdef Q_OS_MACOS
    const std::array keys{QStringLiteral("command"), QStringLiteral("control"),
                          QStringLiteral("option"), QStringLiteral("shift")};
#else
    const std::array keys{QStringLiteral("windows"), QStringLiteral("ctrl"), QStringLiteral("alt"),
                          QStringLiteral("shift")};
#endif
    const std::array buttons{QStringLiteral("left_drag"), QStringLiteral("right_drag"),
                             QStringLiteral("wheel_drag"), QStringLiteral("side_button_1_drag"),
                             QStringLiteral("side_button_2_drag")};
    const std::array actions{Action::ScreenshotCopy,      Action::ScreenshotFixed,
                             Action::ScreenshotOcr,       Action::ScreenshotTranslation,
                             Action::ScreenshotQuickSave, Action::ScreenshotSave,
                             Action::ScreenRecording};
    for (const auto& key : keys) {
        for (const auto& button : buttons) {
            for (const auto action : actions) {
                const auto binding = globalMouseBinding(action, {{key}, button});
                require(binding.has_value(), "every settings option must have a native binding");
                GlobalMouseConfiguration config{{*binding}, true};
                GlobalMouseGesture gesture;
                GlobalMouseInput press{
                    InputKind::Press, {-200, 50}, binding->button, binding->modifiers};
                auto extra = press;
                extra.modifiers |= binding->modifiers == GlobalMouseModifier::Control
                                       ? GlobalMouseModifier::Shift
                                       : GlobalMouseModifier::Control;
                require(!gesture.handle(extra, config).event, "extra modifiers must not activate");
                extra.modifiers = {};
                require(!gesture.handle(extra, config).event,
                        "a bare mouse press must pass through");
                const auto begin = gesture.handle(press, config);
                require(begin.consumed && begin.event && begin.event->kind == EventKind::Begin &&
                            begin.event->position == press.position &&
                            begin.event->action == action,
                        "the press must latch its coordinates and action");
                require(begin.activationModifiers == binding->modifiers,
                        "the initiating physical modifiers must be latched for native ownership");
                config.bindings.clear();
                config.captureAvailable = false;
                auto move = gesture.handle({InputKind::Move, {70, 80}}, config);
                require(
                    move.event && move.event->id == begin.event->id &&
                        move.event->kind == EventKind::Update && !move.consumed,
                    "modifier release and configuration changes must preserve the drag and cursor");
                auto finish =
                    gesture.handle({InputKind::Release, {72, 83}, binding->button}, config);
                require(finish.consumed && finish.event &&
                            finish.event->kind == EventKind::Finish &&
                            finish.event->id == begin.event->id && finish.event->action == action &&
                            finish.event->position == QPoint(72, 83),
                        "initiating release must carry the final position and original action");
                require(
                    !gesture.handle({InputKind::Release, {72, 83}, binding->button}, config).event,
                    "duplicate releases must never execute twice");
                auto cancel = gesture.handle({InputKind::Cancel, {}, Qt::NoButton}, config);
                require(cancel.event && cancel.event->id == begin.event->id,
                        "Escape must cancel a released gesture still awaiting capture readiness");
            }
        }
    }
    require(!globalMouseBinding(Action::ScreenshotCopy, {}), "unset combinations must not bind");
    require(!globalMouseBinding(Action::ScreenshotCopy, {{QStringLiteral("bad")}, buttons[0]}),
            "invalid modifiers must not bind");
    require(!globalMouseBinding(Action::ScreenshotCopy, {{keys[0]}, QStringLiteral("bad")}),
            "invalid buttons must not bind");
}

void multipleKeysMustBeHeldTogetherOnlyAtActivation() {
#ifdef Q_OS_MACOS
    const std::array keys{QStringLiteral("control"), QStringLiteral("shift"),
                          QStringLiteral("option"), QStringLiteral("command")};
#else
    const std::array keys{QStringLiteral("ctrl"), QStringLiteral("shift"), QStringLiteral("alt"),
                          QStringLiteral("windows")};
#endif
    const std::array modifiers{GlobalMouseModifier::Control, GlobalMouseModifier::Shift,
                               GlobalMouseModifier::Alt,
#ifdef Q_OS_MACOS
                               GlobalMouseModifier::Command};
#else
                               GlobalMouseModifier::Super};
#endif
    for (int configuredMask = 1; configuredMask < 16; ++configuredMask) {
        QStringList selected;
        for (int bit = 0; bit < 4; ++bit) {
            if ((configuredMask & (1 << bit)) != 0) {
                selected.push_back(keys.at(static_cast<std::size_t>(bit)));
            }
        }
        const auto binding = globalMouseBinding(Action::ScreenshotTranslation,
                                                {selected, QStringLiteral("left_drag")});
        require(binding.has_value(), "every nonempty key subset must bind");
        const GlobalMouseConfiguration config{{*binding}, true};
        for (int heldMask = 0; heldMask < 16; ++heldMask) {
            GlobalMouseGesture gesture;
            GlobalMouseModifiers held;
            for (int bit = 0; bit < 4; ++bit) {
                if ((heldMask & (1 << bit)) != 0) {
                    held |= modifiers.at(static_cast<std::size_t>(bit));
                }
            }
            const auto result =
                gesture.handle({InputKind::Press, {20, 30}, Qt::LeftButton, held}, config);
            require(result.event.has_value() == (configuredMask == heldMask),
                    "only the complete simultaneous configured chord may activate");
            if (result.event) {
                const auto move = gesture.handle({InputKind::Move, {50, 70}}, {});
                const auto finish =
                    gesture.handle({InputKind::Release, {60, 80}, Qt::LeftButton}, {});
                require(
                    move.event && finish.event && finish.event->id == result.event->id &&
                        finish.event->action == Action::ScreenshotTranslation,
                    "activation must latch the action despite key release and settings changes");
            }
        }
    }
}

void directButtonDragPreservesQtInputAndLatchesAction() {
    GlobalMouseGesture gesture;
    const auto begin = gesture.beginButtonDrag(Action::ScreenshotOcr, {-20, 30});
    require(begin.event && !begin.consumed && begin.activationModifiers == GlobalMouseModifiers{} &&
                begin.event->action == Action::ScreenshotOcr &&
                begin.event->position == QPoint(-20, 30),
            "direct drags must start without activation keys and preserve the Qt press");
    require(!gesture.beginButtonDrag(Action::ScreenshotCopy, {}).event,
            "another action must not replace a pending drag");
    require(gesture.handle({InputKind::Move, {50, 80}}, {}).event.has_value(),
            "direct drag movement must survive disabled global capture settings");
    const auto finish = gesture.handle({InputKind::Release, {60, 90}, Qt::LeftButton}, {});
    require(finish.event && finish.event->kind == EventKind::Finish && !finish.consumed &&
                finish.event->id == begin.event->id &&
                finish.event->action == Action::ScreenshotOcr,
            "the original action must finish once and forward its balanced Qt release");
    require(!gesture.handle({InputKind::Release, {60, 90}, Qt::LeftButton}, {}).event,
            "duplicate local releases must not execute twice");
    gesture.cancel(begin.event->id);
    const auto next = gesture.beginButtonDrag(Action::ScreenshotFixed, {});
    require(next.event.has_value(), "a completed local drag must allow another gesture");
    gesture.cancel(next.event->id);
    require(!gesture.handle({InputKind::Release, {}, Qt::LeftButton}, {}).consumed,
            "cancelled local drags must still forward the Qt release");
}

void suppressionAndCancellationPreserveInputPairs() {
    auto config = configured();
    GlobalMouseGesture gesture;
    const GlobalMouseInput press{
        InputKind::Press, {10, 10}, Qt::LeftButton, GlobalMouseModifier::Control};
    config.captureAvailable = false;
    require(!gesture.handle(press, config).consumed, "busy capture must leave desktop input alone");
    config.captureAvailable = true;
    config.bindings.push_back(
        {Action::ScreenshotFixed, GlobalMouseModifier::Control, Qt::LeftButton});
    require(!gesture.handle(press, config).event, "ambiguous persisted bindings must fail closed");
    config.bindings.removeLast();
    auto injected = press;
    injected.injected = true;
    require(!gesture.handle(injected, config).event, "injected input must not start a capture");
    auto chord = press;
    chord.heldButtons = Qt::RightButton;
    require(!gesture.handle(chord, config).consumed,
            "global mouse must not take over another application drag already in progress");
    const auto begin = gesture.handle(press, config);
    require(begin.event.has_value(), "valid gesture must start");
    require(gesture.handle({InputKind::Press, {}, Qt::BackButton}, config).consumed,
            "additional button presses must be swallowed during the drag");
    require(!gesture.handle({InputKind::Release, {}, Qt::BackButton}, config).event,
            "another button cannot finish the drag");
    gesture.cancel(begin.event->id + 1);
    require(gesture.active(), "stale cancellation must not touch the active gesture");
    gesture.cancel(begin.event->id);
    require(!gesture.active(), "cancellation must stop drag notifications");
    const auto release = gesture.handle({InputKind::Release, {}, Qt::LeftButton}, config);
    require(release.consumed && !release.event, "cancel must drain the swallowed initiating press");
    require(gesture.handle(press, config).event.has_value(),
            "a fresh gesture must work after cancellation");
}

void selectionRetainsThePhysicalAnchorUntilReady() {
    ScreenshotGlobalMouseDrag drag;
    drag.begin(1, {-100, 50}, GlobalMouseCoordinateSpace::DesktopPoints);
    require(drag.coordinateSpace() == GlobalMouseCoordinateSpace::DesktopPoints,
            "macOS desktop coordinates must retain their unit while capture is pending");
    drag.reset();
    require(drag.coordinateSpace() == GlobalMouseCoordinateSpace::PhysicalPixels,
            "reset must discard the previous coordinate space");
    drag.begin(42, {-500, -300});
    require(drag.update(42, {800, 600}) && !drag.ready(), "early moves must be buffered");
    require(drag.update(42, {750, 550}, true), "early release must be buffered");
    require(!drag.update(42, {900, 900}) && !drag.update(41, {0, 0}, true),
            "late moves and stale events must not change the release point");
    drag.setReady();
    require(drag.start() == QPoint(-500, -300) && drag.end() == QPoint(750, 550) && drag.released(),
            "capture readiness must preserve the original anchor and final release");
    const QRectF bounds(-600, -400, 2400, 1600);
    for (const QPointF end :
         {QPointF(-550, -350), QPointF(-450, -350), QPointF(-550, -250), QPointF(-450, -250)}) {
        const QRectF selection = ScreenshotGlobalMouseDrag::selection(drag.start(), end, bounds);
        require(selection.size() == QSizeF(50, 50),
                "all drag directions must normalize consistently");
    }
    require(ScreenshotGlobalMouseDrag::selection({0, 0}, {1, 1}, bounds).size() == QSizeF(1, 1),
            "one physical pixel in both dimensions is usable");
    require(ScreenshotGlobalMouseDrag::selection({0, 0}, {0, 30}, bounds).isEmpty() &&
                ScreenshotGlobalMouseDrag::selection({0, 0}, {30, 0}, bounds).isEmpty() &&
                ScreenshotGlobalMouseDrag::selection({0, 0}, {0, 0}, bounds).isEmpty(),
            "empty and single-axis gestures must not expand to a usable selection");
    require(ScreenshotGlobalMouseDrag::selection({-900, -900}, {9000, 9000}, bounds) == bounds,
            "selection endpoints must stay within the captured desktop");
    drag.reset();
    drag.setReady();
    require(!drag.active() && !drag.update(42, {0, 0}, true),
            "cancelled sessions cannot be revived");
}

void revealRefreshCatchesUpToLiveCursorOnlyWhileDragging() {
    ScreenshotGlobalMouseDrag drag;
    drag.begin(7, {100, 100});
    require(drag.update(7, {200, 200}) && !drag.ready(),
            "a pending drag must buffer paced movement before readiness");
    drag.setReady();
    drag.refreshEndFromLivePosition(QPoint(319, 407));
    require(drag.start() == QPoint(100, 100) && drag.end() == QPoint(319, 407) && !drag.released(),
            "activation must adopt the live cursor instead of the last paced event");
    drag.refreshEndFromLivePosition(std::nullopt);
    require(drag.end() == QPoint(319, 407),
            "an unavailable cursor position must keep the buffered end point");
    require(drag.update(7, {400, 400}, true), "a release after activation must buffer normally");
    drag.refreshEndFromLivePosition(QPoint(900, 900));
    require(drag.end() == QPoint(400, 400) && drag.released(),
            "a buffered release must never be overridden by the live cursor");
    ScreenshotGlobalMouseDrag inactive;
    inactive.refreshEndFromLivePosition(QPoint(5, 5));
    require(!inactive.active() && inactive.end() == QPoint(),
            "a reset drag must ignore live-cursor refreshes");
}

class FakeBackend final : public GlobalMouseBackend {
  public:
    void start(Handler callback, FailureHandler error) override {
        handler = std::move(callback);
        failure = std::move(error);
        ++starts;
    }
    void stop() override {
        ++stops;
    }
    void configure(const GlobalMouseConfiguration& value) override {
        configuration = value;
    }
    void cancel(quint64 id) override {
        cancelled = id;
    }
    void beginButtonDrag(Action action) override {
        buttonAction = action;
        ++buttonDrags;
    }
    void setStateHandler(StateHandler value) override {
        stateHandler = std::move(value);
    }
    StateHandler stateHandler;
    Action buttonAction = Action::ScreenshotCopy;
    int buttonDrags = 0;
    Handler handler;
    FailureHandler failure;
    GlobalMouseConfiguration configuration;
    quint64 cancelled = 0;
    int starts = 0;
    int stops = 0;
};

class FakeHotkeyBackend final : public GlobalShortcutBackend {
  public:
    void setActivationHandler(ActivationHandler value) override {
        handler = std::move(value);
    }
    GlobalShortcutValidationResult
    validateShortcut(const snow_shot::shortcuts::ShortcutBinding& shortcut) const override {
        return {shortcut.portableText, true, GlobalShortcutFailureReason::None, shortcut};
    }
    GlobalShortcutBackendResult
    registerShortcut(int id, const snow_shot::shortcuts::ShortcutBinding&) override {
        registrationId = id;
        return {true};
    }
    void unregisterShortcut(int) override {}

    ActivationHandler handler;
    int registrationId = 0;
};

void hotkeySuppressionNeverDisablesMouseGestures() {
    QTemporaryDir temporary;
    require(temporary.isValid(), "test storage directory must be available");
    auto& storage = snow_shot::storage::ApplicationStorage::instance();
    static_cast<void>(
        storage.initialize({temporary.filePath(QStringLiteral("bin")), temporary.path()}));
    require(storage.isInitialized(), "test storage must initialize");
    auto& store = storage.configuration();
    const QString fullscreenKey =
        QStringLiteral("global_shortcuts/disable_on_focused_fullscreen_window");
    require(store.setValue(fullscreenKey, true), "initial fullscreen suppression must persist");
    require(
        store.setValue(QStringLiteral("global_mouse/screenshot_copy"),
                       QJsonObject{{QStringLiteral("activation_key"),
                                    QJsonArray{globalMouseActivationKeys().at(0)}},
                                   {QStringLiteral("mouse_button"), QStringLiteral("left_drag")}}),
        "configure a mouse binding independently of platform defaults");
    {
        bool fullscreen = true;
        auto hotkeyBackend = std::make_unique<FakeHotkeyBackend>();
        auto* hotkeyInput = hotkeyBackend.get();
        GlobalShortcutManager hotkeys(std::move(hotkeyBackend), nullptr,
                                      [&fullscreen]() { return fullscreen; });
        hotkeys.setShortcuts(GlobalShortcutAction::Screenshot, {QStringLiteral("Ctrl+F9")});
        int hotkeyActivations = 0;
        QObject::connect(&hotkeys, &GlobalShortcutManager::activated, &hotkeys,
                         [&hotkeyActivations](GlobalShortcutAction) { ++hotkeyActivations; });
        auto mouseBackend = std::make_unique<FakeBackend>();
        auto* mouseInput = mouseBackend.get();
        GlobalMouseManager mouse(std::move(mouseBackend));
        mouse.setCaptureAvailable(true);
        mouse.initialize();
        const auto requireMouseActivation = [&]() {
            GlobalMouseGesture gesture;
            const GlobalMouseInput press{InputKind::Press,
                                         {10, 20},
                                         Qt::LeftButton,
#ifdef Q_OS_MACOS
                                         GlobalMouseModifier::Command};
#else
                                         GlobalMouseModifier::Super};
#endif
            const auto result = gesture.handle(press, mouseInput->configuration);
            require(result.consumed && result.event &&
                        result.event->action == Action::ScreenshotCopy,
                    "hotkey suppression must leave configured mouse gestures active");
        };
        requireMouseActivation();
        for (const bool enabled : {false, true}) {
            hotkeys.setGlobalHotkeysEnabled(enabled);
            for (const bool suppressFullscreen : {false, true}) {
                require(store.setValue(fullscreenKey, suppressFullscreen),
                        "fullscreen suppression changes must persist");
                for (const bool focusedFullscreen : {false, true}) {
                    fullscreen = focusedFullscreen;
                    const int previousActivations = hotkeyActivations;
                    hotkeyInput->handler(hotkeyInput->registrationId);
                    require(hotkeyActivations - previousActivations ==
                                static_cast<int>(enabled && !(suppressFullscreen && fullscreen)),
                            "global hotkeys must obey manual and fullscreen suppression");
                    requireMouseActivation();
                }
            }
        }
        mouse.setCaptureAvailable(false);
        GlobalMouseGesture gesture;
        require(!gesture
                     .handle({InputKind::Press,
                              {},
                              Qt::LeftButton,
#ifdef Q_OS_MACOS
                              GlobalMouseModifier::Command},
#else
                              GlobalMouseModifier::Super},
#endif
                             mouseInput->configuration)
                     .event,
                "busy capture must still block mouse activation");
    }
    storage.shutdown();
}

void managerLoadsLiveSettingsAndCoalescesOnlyMovement() {
    QTemporaryDir temporary;
    require(temporary.isValid(), "test storage directory must be available");
    auto& storage = snow_shot::storage::ApplicationStorage::instance();
    static_cast<void>(
        storage.initialize({temporary.filePath(QStringLiteral("bin")), temporary.path()}));
    require(storage.isInitialized(), "test storage must initialize");
    auto backend = std::make_unique<FakeBackend>();
    auto* input = backend.get();
    GlobalMouseManager manager(std::move(backend));
    manager.setCaptureAvailable(true);
    manager.initialize();
    manager.initialize();
#ifdef Q_OS_MACOS
    require(input->starts == 1 && input->configuration.bindings.isEmpty(),
            "macOS initialization must be idempotent with no default mouse bindings");
#else
    require(input->starts == 1 && input->configuration.bindings.size() == 3,
            "initialization must be idempotent and load three default bindings");
    const std::array defaultActions{Action::ScreenshotCopy, Action::ScreenshotFixed,
                                    Action::ScreenshotOcr};
    const std::array defaultButtons{Qt::LeftButton, Qt::MiddleButton, Qt::RightButton};
    for (qsizetype index = 0; index < 3; ++index) {
        const auto& binding = input->configuration.bindings[index];
        require(binding.action == defaultActions.at(static_cast<std::size_t>(index)) &&
                    binding.modifiers == GlobalMouseModifier::Super &&
                    binding.button == defaultButtons.at(static_cast<std::size_t>(index)),
                "default bindings must reach the backend as Windows plus left, middle, and right");
    }
#endif
    for (const auto* key : {"global_mouse/screenshot_copy", "global_mouse/screenshot_fixed",
                            "global_mouse/screenshot_ocr"}) {
        require(storage.configuration().setValue(QString::fromLatin1(key), QJsonObject{}),
                "explicitly clearing a default must be accepted");
    }
    manager.beginButtonDrag(Action::ScreenshotOcr);
    require(input->buttonDrags == 1 && input->buttonAction == Action::ScreenshotOcr,
            "direct button drags must work with unset bindings");
    manager.setCaptureAvailable(false);
    manager.beginButtonDrag(Action::ScreenshotFixed);
    require(input->buttonDrags == 1, "busy capture must reject a direct button drag");
    manager.setCaptureAvailable(true);
    auto& store = storage.configuration();
    require(
        store.setValue(QStringLiteral("global_mouse/screenshot_quick_save"),
                       QJsonObject{{QStringLiteral("activation_key"), QStringLiteral("shift")},
                                   {QStringLiteral("mouse_button"), QStringLiteral("wheel_drag")}}),
        "test combination must persist");
    require(input->configuration.bindings.size() == 1 &&
                input->configuration.bindings[0].action == Action::ScreenshotQuickSave,
            "committed settings must update the backend immediately");
    require(
        store.setValue(QStringLiteral("global_mouse/screenshot_quick_save"),
                       QJsonObject{{QStringLiteral("activation_key"),
#ifdef Q_OS_MACOS
                                    QJsonArray{QStringLiteral("control"), QStringLiteral("shift")}},
#else
                                    QJsonArray{QStringLiteral("ctrl"), QStringLiteral("shift")}},
#endif
                                   {QStringLiteral("mouse_button"), QStringLiteral("wheel_drag")}}),
        "multiple activation keys must persist");
    require(input->configuration.bindings[0].modifiers ==
#ifdef Q_OS_MACOS
                (GlobalMouseModifier::Control | GlobalMouseModifier::Shift),
#else
                (GlobalMouseModifier::Control | GlobalMouseModifier::Shift),
#endif
            "all persisted activation keys must reach the native backend");
    require(store.setValue(
                QStringLiteral("global_mouse/screen_recording"),
#ifdef Q_OS_MACOS
                QJsonObject {
                    {QStringLiteral("activation_key"), QJsonArray{QStringLiteral("option")}},
#else
                QJsonObject{{QStringLiteral("activation_key"), QJsonArray{QStringLiteral("alt")}},
#endif
                        {
                        QStringLiteral("mouse_button"), QStringLiteral("left_drag")
                    }
                }),
            "screen recording combination must persist");
    require(input->configuration.bindings.size() == 2 &&
                input->configuration.bindings.last().action == Action::ScreenRecording &&
                input->configuration.bindings.last().modifiers == GlobalMouseModifier::Alt &&
                input->configuration.bindings.last().button == Qt::LeftButton,
            "screen recording settings must reach the native backend immediately");
    manager.setCaptureAvailable(false);
    require(!input->configuration.captureAvailable,
            "capture availability must reach the native backend");
    QVector<GlobalMouseDragEvent> delivered;
    QObject::connect(&manager, &GlobalMouseManager::dragEvent, &manager,
                     [&delivered](const auto& event) { delivered.push_back(event); });
    int failures = 0;
    QObject::connect(&manager, &GlobalMouseManager::operationFailed, &manager,
                     [&failures](const QString&) { ++failures; });
    input->handler({EventKind::Begin, 7, Action::ScreenshotQuickSave, {10, 20}});
    for (int i = 0; i < 1000; ++i) {
        input->handler({EventKind::Update, 7, Action::ScreenshotQuickSave, QPoint(i, 25)});
    }
    input->handler({EventKind::Finish, 7, Action::ScreenshotQuickSave, {1100, 30}});
    input->failure(123);
    QCoreApplication::processEvents();
    require(delivered.size() == 2 && delivered[0].position == QPoint(10, 20) &&
                delivered[1].kind == EventKind::Finish && delivered[1].position == QPoint(1100, 30),
            "the exact release must supersede movement not yet presented");
    require(failures == 1, "backend failure must notify the application");
    manager.cancelGesture(7);
    require(input->cancelled == 7, "capture cancellation must reach native gesture ownership");
    require(store.setValue(QStringLiteral("global_mouse/screenshot_quick_save"), QJsonObject{}),
            "reset must persist");
    require(store.setValue(QStringLiteral("global_mouse/screen_recording"), QJsonObject{}),
            "screen recording reset must persist");
    require(input->configuration.bindings.isEmpty(), "reset must remove runtime bindings");
    input->handler({EventKind::Begin, 8, Action::ScreenshotCopy, {}});
    manager.shutdown();
    QCoreApplication::processEvents();
    require(input->stops == 1 && delivered.size() == 2, "shutdown must discard queued input");
    storage.shutdown();
}

void managerRetainsLatestMovementUntilPresentationConsumesIt() {
    QTemporaryDir temporary;
    auto& storage = snow_shot::storage::ApplicationStorage::instance();
    static_cast<void>(
        storage.initialize({temporary.filePath(QStringLiteral("bin")), temporary.path()}));
    require(storage.isInitialized(), "mailbox test storage must initialize");
    {
        auto backend = std::make_unique<FakeBackend>();
        auto* input = backend.get();
        GlobalMouseManager manager(std::move(backend));
        manager.initialize();
        require(manager.findChildren<QTimer*>().isEmpty(),
                "the manager must leave frame scheduling to the presentation owner");
        QVector<GlobalMouseDragEvent> delivered;
        QVector<quint64> notifications;
        QObject::connect(&manager, &GlobalMouseManager::dragEvent, &manager,
                         [&delivered](const auto& event) { delivered.push_back(event); });
        QObject::connect(&manager, &GlobalMouseManager::dragUpdateAvailable, &manager,
                         [&notifications](quint64 id) { notifications.push_back(id); });
        const auto drain = [&] { QCoreApplication::sendPostedEvents(&manager, QEvent::MetaCall); };
        input->handler({EventKind::Begin, 1, Action::ScreenshotCopy, {10, 20}});
        input->handler({EventKind::Update, 1, Action::ScreenshotCopy, {11, 21}});
        require(!manager.takePendingUpdate(1),
                "movement must not be consumable before its Begin reaches the GUI");
        drain();
        require(delivered.size() == 1 && delivered.back().kind == EventKind::Begin &&
                    notifications == QVector<quint64>{1},
                "Begin must precede the one pending movement notification");
        for (int frame = 0; frame < 3; ++frame) {
            for (int i = 0; i < 1000; ++i) {
                input->handler({EventKind::Update, 1, Action::ScreenshotCopy, QPoint(i, frame)});
                drain();
            }
            require(delivered.size() == 1 && notifications.size() == frame + 1,
                    "raw movement must retain its notification latch throughout a pending frame");
            const auto update = manager.takePendingUpdate(1);
            require(update && update->kind == EventKind::Update && update->id == 1 &&
                        update->position == QPoint(999, frame) && !manager.takePendingUpdate(1),
                    "each presentation frame must consume exactly the newest position");
            drain();
            require(notifications.size() == frame + 1,
                    "consumption must not wake an idle presentation owner");
        }
        input->handler({EventKind::Update, 1, Action::ScreenshotCopy, {1001, 4}});
        input->handler({EventKind::Finish, 1, Action::ScreenshotCopy, {1002, 5}});
        require(!manager.takePendingUpdate(1),
                "publication of Finish must supersede unread movement before GUI delivery");
        drain();
        require(delivered.size() == 2 && delivered.back().kind == EventKind::Finish &&
                    delivered.back().position == QPoint(1002, 5) && notifications.size() == 3,
                "Finish must deliver its exact position without waiting for a presentation frame");
        input->handler({EventKind::Update, 1, Action::ScreenshotCopy, {2000, 6}});
        drain();
        require(!manager.takePendingUpdate(1) && notifications.size() == 3,
                "movement published after release must not revive its mailbox");
        input->handler({EventKind::Cancel, 1, Action::ScreenshotCopy, {}});
        drain();
        require(delivered.size() == 3 && delivered.back().kind == EventKind::Cancel,
                "a released capture must remain cancellable while preparation is pending");
        manager.cancelGesture(1);
        input->handler({EventKind::Cancel, 1, Action::ScreenshotCopy, {}});
        drain();
        require(delivered.size() == 3, "acknowledged cancellation must reject stale events");
        input->handler({EventKind::Begin, 2, Action::ScreenshotCopy, {}});
        input->handler({EventKind::Update, 2, Action::ScreenshotCopy, {5, 5}});
        input->handler({EventKind::Cancel, 2, Action::ScreenshotCopy, {}});
        drain();
        require(delivered.size() == 5 && delivered.at(3).kind == EventKind::Begin &&
                    delivered.back().kind == EventKind::Cancel && notifications.size() == 3 &&
                    !manager.takePendingUpdate(2),
                "Cancel before notification must preserve boundaries and discard its movement");
        input->handler({EventKind::Begin, 3, Action::ScreenshotCopy, {}});
        drain();
        input->handler({EventKind::Update, 3, Action::ScreenshotCopy, {6, 6}});
        drain();
        require(notifications.size() == 4 && notifications.back() == 3,
                "movement after an idle frame must request presentation once");
        input->handler({EventKind::Finish, 3, Action::ScreenshotCopy, {7, 7}});
        drain();
        require(delivered.back().kind == EventKind::Finish &&
                    delivered.back().position == QPoint(7, 7) && !manager.takePendingUpdate(3),
                "Finish after notification must invalidate the unconsumed sample");
        input->handler({EventKind::Begin, 4, Action::ScreenshotCopy, {}});
        drain();
        input->handler({EventKind::Update, 4, Action::ScreenshotCopy, {8, 8}});
        drain();
        require(manager.takePendingUpdate(4).has_value(), "the frame must consume active movement");
        input->handler({EventKind::Finish, 4, Action::ScreenshotCopy, {9, 9}});
        drain();
        require(delivered.back().kind == EventKind::Finish &&
                    delivered.back().position == QPoint(9, 9),
                "Finish after frame consumption must still retain its exact release position");
        input->handler({EventKind::Begin, 5, Action::ScreenshotCopy, {}});
        drain();
        input->handler({EventKind::Update, 5, Action::ScreenshotCopy, {10, 10}});
        drain();
        input->handler({EventKind::Cancel, 5, Action::ScreenshotCopy, {}});
        require(!manager.takePendingUpdate(5),
                "Cancel must invalidate notified movement immediately");
        drain();
        require(delivered.back().kind == EventKind::Cancel,
                "Cancel after notification must remain an immediate boundary");
        input->handler({EventKind::Begin, 6, Action::ScreenshotCopy, {}});
        drain();
        input->handler({EventKind::Update, 6, Action::ScreenshotCopy, {11, 11}});
        drain();
        const auto beforeCancellation = notifications.size();
        manager.cancelGesture(6);
        input->handler({EventKind::Update, 6, Action::ScreenshotCopy, {12, 12}});
        drain();
        require(!manager.takePendingUpdate(6) && notifications.size() == beforeCancellation,
                "controller cancellation must discard movement and reject later producer updates");

        std::optional<GlobalMouseDragEvent> startupUpdate;
        const auto startupConsumer = QObject::connect(
            &manager, &GlobalMouseManager::dragEvent, &manager, [&](const auto& event) {
                if (event.id == 7 && event.kind == EventKind::Begin)
                    startupUpdate = manager.takePendingUpdate(event.id);
            });
        input->handler({EventKind::Begin, 7, Action::ScreenshotCopy, {}});
        input->handler({EventKind::Update, 7, Action::ScreenshotCopy, {13, 13}});
        drain();
        require(startupUpdate && startupUpdate->position == QPoint(13, 13) &&
                    notifications.size() == beforeCancellation,
                "startup must consume its latest sample without leaving a stale notification");
        QObject::disconnect(startupConsumer);
        input->handler({EventKind::Update, 7, Action::ScreenshotCopy, {14, 14}});
        drain();
        manager.cancelGesture(7);
        require(!manager.takePendingUpdate(7), "cancel must retire a sample held during startup");

        const auto oldHandler = input->handler;
        const auto oldFailure = input->failure;
        int failures = 0;
        QObject::connect(&manager, &GlobalMouseManager::operationFailed, &manager,
                         [&failures](const QString&) { ++failures; });
        oldHandler({EventKind::Begin, 8, Action::ScreenshotCopy, {}});
        oldHandler({EventKind::Update, 8, Action::ScreenshotCopy, {15, 15}});
        oldFailure(1);
        manager.shutdown();
        manager.initialize();
        const auto count = delivered.size();
        oldHandler({EventKind::Update, 8, Action::ScreenshotCopy, {16, 16}});
        input->handler({EventKind::Begin, 9, Action::ScreenshotCopy, {17, 17}});
        input->handler({EventKind::Update, 9, Action::ScreenshotCopy, {18, 18}});
        drain();
        const auto restartedUpdate = manager.takePendingUpdate(9);
        require(delivered.size() == count + 1 && delivered.back().id == 9 && failures == 0 &&
                    !manager.takePendingUpdate(8) && restartedUpdate &&
                    restartedUpdate->position == QPoint(18, 18),
                "restart must reject stale callbacks and movement without losing new input");

        QObject::connect(&manager, &GlobalMouseManager::dragEvent, &manager,
                         [&](const auto& event) {
                             if (event.id == 10 && event.kind == EventKind::Begin) {
                                 manager.shutdown();
                                 manager.initialize();
                             }
                         });
        input->handler({EventKind::Begin, 10, Action::ScreenshotCopy, {}});
        input->handler({EventKind::Update, 10, Action::ScreenshotCopy, {19, 19}});
        input->handler({EventKind::Finish, 10, Action::ScreenshotCopy, {20, 20}});
        drain();
        require(delivered.back().id == 10 && delivered.back().kind == EventKind::Cancel &&
                    !manager.takePendingUpdate(10),
                "reentrant restart must stop the old boundary batch and discard its mailbox");

        const auto reentrantCancel = QObject::connect(
            &manager, &GlobalMouseManager::dragEvent, &manager, [&](const auto& event) {
                if (event.id == 11 && event.kind == EventKind::Begin)
                    manager.cancelGesture(event.id);
            });
        input->handler({EventKind::Begin, 11, Action::ScreenshotCopy, {}});
        input->handler({EventKind::Update, 11, Action::ScreenshotCopy, {21, 21}});
        input->handler({EventKind::Finish, 11, Action::ScreenshotCopy, {22, 22}});
        drain();
        require(delivered.back().id == 11 && delivered.back().kind == EventKind::Begin &&
                    input->cancelled == 11 && !manager.takePendingUpdate(11),
                "cancelling in Begin must reject later boundaries in the same delivery batch");
        QObject::disconnect(reentrantCancel);

        const auto reentrantFinish = QObject::connect(
            &manager, &GlobalMouseManager::dragEvent, &manager, [&](const auto& event) {
                if (event.id == 12 && event.kind == EventKind::Begin)
                    input->handler({EventKind::Finish, 12, Action::ScreenshotCopy, {23, 23}});
            });
        input->handler({EventKind::Begin, 12, Action::ScreenshotCopy, {}});
        input->handler({EventKind::Update, 12, Action::ScreenshotCopy, {22, 22}});
        drain();
        drain();
        require(delivered.back().id == 12 && delivered.back().kind == EventKind::Finish &&
                    delivered.back().position == QPoint(23, 23) && !manager.takePendingUpdate(12),
                "Finish published during Begin must retire movement and follow Begin in order");
        QObject::disconnect(reentrantFinish);

        input->handler({EventKind::Begin, 13, Action::ScreenshotCopy, {24, 24}});
        input->handler({EventKind::Finish, 13, Action::ScreenshotCopy, {25, 25}});
        input->handler({EventKind::Begin, 14, Action::ScreenshotCopy, {26, 26}});
        input->handler({EventKind::Update, 14, Action::ScreenshotCopy, {27, 27}});
        const auto beforeNewGesture = delivered.size();
        drain();
        const auto nextGestureUpdate = manager.takePendingUpdate(14);
        require(delivered.size() == beforeNewGesture + 3 &&
                    delivered.at(beforeNewGesture).id == 13 &&
                    delivered.at(beforeNewGesture + 1).kind == EventKind::Finish &&
                    delivered.back().id == 14 && nextGestureUpdate &&
                    nextGestureUpdate->position == QPoint(27, 27),
                "queued gesture boundaries must precede notification of the newest gesture");
        input->handler({EventKind::Finish, 14, Action::ScreenshotCopy, {28, 28}});
        drain();
        const auto completedCount = delivered.size();
        input->handler({EventKind::Update, 14, Action::ScreenshotCopy, {29, 29}});
        drain();
        require(delivered.size() == completedCount && !manager.takePendingUpdate(14),
                "released captures must reject movement while remaining cancellable");
        input->handler({EventKind::Cancel, 14, Action::ScreenshotCopy, {}});
        manager.cancelGesture(14);
        drain();
        require(delivered.size() == completedCount,
                "completion acknowledgement must discard queued cancellation after release");
        input->handler({EventKind::Begin, 15, Action::ScreenshotCopy, {}});
        drain();
        input->handler({EventKind::Update, 15, Action::ScreenshotCopy, {30, 30}});
        drain();
        require(manager.takePendingUpdate(15).has_value(),
                "a frame must consume movement before the cancellation boundary");
        input->handler({EventKind::Cancel, 15, Action::ScreenshotCopy, {}});
        drain();
        require(delivered.back().id == 15 && delivered.back().kind == EventKind::Cancel &&
                    !manager.takePendingUpdate(15),
                "Cancel after consumption must retire the gesture without replaying movement");
    }
    storage.shutdown();
}

void managerConsumesMovementPublishedAcrossThreadsWithoutLosingWakeups() {
    QTemporaryDir temporary;
    auto& storage = snow_shot::storage::ApplicationStorage::instance();
    static_cast<void>(
        storage.initialize({temporary.filePath(QStringLiteral("bin")), temporary.path()}));
    require(storage.isInitialized(), "cross-thread mailbox storage must initialize");
    {
        auto backend = std::make_unique<FakeBackend>();
        auto* input = backend.get();
        GlobalMouseManager manager(std::move(backend));
        manager.initialize();
        int notifications = 0;
        QObject::connect(&manager, &GlobalMouseManager::dragUpdateAvailable, &manager,
                         [&](quint64 id) {
                             require(id == 1, "cross-thread movement must retain its gesture");
                             ++notifications;
                         });
        const auto drain = [&] { QCoreApplication::sendPostedEvents(&manager, QEvent::MetaCall); };
        input->handler({EventKind::Begin, 1, Action::ScreenshotCopy, {}});
        drain();
        const auto publish = input->handler;
        QSemaphore startRound;
        QSemaphore completedRound;
        constexpr int rounds = 64;
        std::thread producer([&] {
            for (int round = 0; round < rounds; ++round) {
                startRound.acquire();
                for (int position = 0; position < 1000; ++position)
                    publish(
                        {EventKind::Update, 1, Action::ScreenshotCopy, QPoint(position, round)});
                completedRound.release();
            }
        });
        for (int round = 0; round < rounds; ++round) {
            publish({EventKind::Update, 1, Action::ScreenshotCopy, QPoint(-1, round)});
            drain();
            const int beforeRace = notifications;
            startRound.release();
            const auto racingUpdate = manager.takePendingUpdate(1);
            require(racingUpdate.has_value(), "a frame must consume the pending sample atomically");
            require(completedRound.tryAcquire(1, 10000), "the producer burst must complete");
            drain();
            const auto latest = manager.takePendingUpdate(1);
            const auto finalPosition = latest ? latest->position : racingUpdate->position;
            require(finalPosition == QPoint(999, round) && notifications <= beforeRace + 1,
                    "publication racing consumption must retain the newest sample with one wake");
            drain();
            require(!manager.takePendingUpdate(1), "a drained frame must not replay its movement");
        }
        producer.join();
        manager.shutdown();
    }
    storage.shutdown();
}
void managerPermissionCallbacksAreRestartSafe() {
    QTemporaryDir temporary;
    auto& storage = snow_shot::storage::ApplicationStorage::instance();
    static_cast<void>(
        storage.initialize({temporary.filePath(QStringLiteral("bin")), temporary.path()}));
    auto backend = std::make_unique<FakeBackend>();
    auto* input = backend.get();
    GlobalMouseManager manager(std::move(backend));
    int failures = 0;
    QObject::connect(&manager, &GlobalMouseManager::operationFailed, &manager,
                     [&](const QString& message) {
                         require(!message.isEmpty(), "permission failure is actionable");
                         ++failures;
                     });
    manager.initialize();
    const auto stale = input->stateHandler;
    input->stateHandler({GlobalMousePermissionState::Status::ListenRequired});
    QCoreApplication::processEvents();
    require(failures == 1 && manager.permissionState().status ==
                                 GlobalMousePermissionState::Status::ListenRequired,
            "startup permission failure must reach both settings and tray subscribers");
    manager.shutdown();
    manager.initialize();
    stale({GlobalMousePermissionState::Status::Unavailable});
    QCoreApplication::processEvents();
    require(failures == 1 &&
                manager.permissionState().status == GlobalMousePermissionState::Status::Ready,
            "stale permission callbacks must not overwrite a restarted backend");
    manager.shutdown();
    storage.shutdown();
}

} // namespace

int main(int argc, char** argv) {
    QApplication app(argc, argv);
    if (app.arguments().contains(QStringLiteral("--native-hook-smoke"))) {
#ifdef Q_OS_WIN
        require(QGuiApplication::platformName() == QStringLiteral("windows"),
                "native smoke test requires the Windows platform");
        auto backend = createGlobalMouseBackend();
        backend->configure({});
        std::atomic_int failures = 0;
        for (int iteration = 0; iteration < 2; ++iteration) {
            backend->start([](const GlobalMouseDragEvent&) {},
                           [&failures](quint32) { ++failures; });
            backend->stop();
        }
        require(failures == 0, "Windows hooks must install and shut down cleanly");
        return 0;
#else
        require(false, "native smoke test is unsupported on this platform");
#endif
    }
    managerPermissionCallbacksAreRestartSafe();
    everyCombinationMatchesExactlyAndLatchesItsAction();
#ifdef Q_OS_WIN
    globalMouseNativeActivationKeyTests();
    globalMouseNativePerformanceTests();
    globalMouseNativeStopFromWorkerThreadTests();
#endif
    multipleKeysMustBeHeldTogetherOnlyAtActivation();
    directButtonDragPreservesQtInputAndLatchesAction();
    suppressionAndCancellationPreserveInputPairs();
    selectionRetainsThePhysicalAnchorUntilReady();
    revealRefreshCatchesUpToLiveCursorOnlyWhileDragging();
    hotkeySuppressionNeverDisablesMouseGestures();
    managerLoadsLiveSettingsAndCoalescesOnlyMovement();
    managerRetainsLatestMovementUntilPresentationConsumesIt();
    managerConsumesMovementPublishedAcrossThreadsWithoutLosingWakeups();
    return 0;
}
