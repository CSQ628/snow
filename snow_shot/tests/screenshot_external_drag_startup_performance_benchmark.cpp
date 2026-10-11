#include "screenshot_selection_presentation_fixture.h"

#include "snow_shot/presentation/screenshotoverlayinputhandler.h"
#include "snow_shot/presentation/screenshotoverlaycanvaspresenter.h"
#include "snow_shot/presentation/globalmousemanager.h"

#include <QCommandLineParser>
#include <QEventLoop>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTimer>
#include <QWindow>

#ifdef Q_OS_WIN
#include <qt_windows.h>
#endif

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <iostream>
#include <map>
#include <mutex>
#include <thread>
#include <vector>

namespace snow_shot::presentation {
// This executable supplies a synthetic backend; never install a physical global mouse hook.
std::unique_ptr<GlobalMouseBackend> createGlobalMouseBackend() {
    return {};
}
} // namespace snow_shot::presentation

namespace {
using namespace snow_shot::presentation;
using selection_presentation_test::IsolatedStorage;
using selection_presentation_test::NoopToolbarCommands;

void require(bool condition, const char* message) {
    if (!condition)
        throw std::runtime_error(message);
}

qint64 wallNanoseconds() {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

qint64 guiCpuNanoseconds() {
#ifdef Q_OS_WIN
    FILETIME creation{}, exit{}, kernel{}, user{};
    if (GetThreadTimes(GetCurrentThread(), &creation, &exit, &kernel, &user)) {
        const auto ticks = [](const FILETIME& value) {
            return (static_cast<quint64>(value.dwHighDateTime) << 32) | value.dwLowDateTime;
        };
        return static_cast<qint64>((ticks(kernel) + ticks(user)) * 100);
    }
#endif
    return 0;
}

double milliseconds(qint64 ns) {
    return static_cast<double>(ns) / 1000000.0;
}

struct Duration {
    qint64 wallNs = 0;
    qint64 cpuNs = 0;
    QJsonObject json() const {
        return {{"wall_ms", milliseconds(wallNs)}, {"gui_cpu_ms", milliseconds(cpuNs)}};
    }
};

template <typename Callback> Duration measure(Callback callback) {
    const qint64 cpu = guiCpuNanoseconds();
    const qint64 started = wallNanoseconds();
    callback();
    return {wallNanoseconds() - started, guiCpuNanoseconds() - cpu};
}

// Native Qt pointer events must not contaminate the producer's synthetic gesture. The activation
// boundary still follows the controller's real sink -> input handler -> service action route.
class StartupOverlayEvents final : public ScreenshotOverlayEventSink {
  public:
    bool shouldHandleOverlayMouseEvent(const ScreenshotOverlayWindow*, const QPointF&,
                                       bool) const override {
        return false;
    }
    void handleOverlayMousePress(ScreenshotOverlayWindow*, const QPointF&) override {}
    void handleOverlayMouseMove(ScreenshotOverlayWindow*, const QPointF&) override {}
    void handleOverlayMouseRelease(ScreenshotOverlayWindow*, const QPointF&) override {}
    ScreenshotOverlayRightClickResult handleOverlayRightClick(ScreenshotOverlayWindow*,
                                                              const QPointF&) override {
        return ScreenshotOverlayRightClickResult::Ignored;
    }
    bool handleOverlayWheel(ScreenshotOverlayWindow*, const QWheelEvent&) override {
        return false;
    }
    bool shouldBlockUnhandledOverlayKeyInput() const override {
        return false;
    }
    void raiseToolbarForCanvasInteraction() override {}
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
    void prepareOverlayActivation() override {
        activationHookNs = wallNanoseconds();
        activationHookDuration = measure(prepareActivation);
    }
#endif
    std::function<void()> prepareActivation = [] {};
    qint64 activationHookNs = 0;
    Duration activationHookDuration;
};

class InputBackend final : public GlobalMouseBackend {
  public:
    void start(Handler callback, FailureHandler) override {
        handler = std::move(callback);
    }
    void stop() override {
        handler = {};
    }
    void configure(const GlobalMouseConfiguration&) override {}
    void cancel(quint64) override {}
    void beginButtonDrag(settings::SettingsGlobalMouseAction) override {}
    Handler handler;
};

// One producer is created before any capture measurement. The real manager owns its pending
// sample and queued notification. Source timestamps remain independent of GUI consumption.
class WorkerProducer final {
  public:
    struct Job {
        QObject* receiver = nullptr;
        GlobalMouseBackend::Handler input;
        std::function<void()> acquired;
        quint64 gestureId = 0;
        QPointF initialEnd;
        QPointF step;
        qint64 acquireDelayNs = 0;
        std::atomic<qint64> dispatchedNs{0};
        std::atomic<qint64> workerReadyNs{0};
        std::atomic<qint64> latestSourceNs{0};
        std::atomic<qint64> latestSequence{0};
        std::atomic<bool> cancelled{false};
        std::mutex samplesMutex;
        std::vector<qint64> sampleTimes{0};
        std::vector<double> sourceGaps;

        qint64 sampleTime(const QPointF& point) {
            const qint64 sequence = sequenceFor(point);
            std::lock_guard lock(samplesMutex);
            require(sequence >= 0 && static_cast<std::size_t>(sequence) < sampleTimes.size(),
                    "manager delivered an unknown source sequence");
            return sampleTimes[static_cast<std::size_t>(sequence)];
        }

        qint64 sequenceFor(const QPointF& point) const {
            return qRound64((point.x() - initialEnd.x()) / step.x());
        }

        std::pair<QPointF, qint64> liveSample() {
            std::lock_guard lock(samplesMutex);
            const auto sequence = static_cast<qint64>(sampleTimes.size() - 1);
            return {initialEnd + step * static_cast<qreal>(sequence), sampleTimes.back()};
        }
    };

    WorkerProducer() {
#ifdef Q_OS_WIN
        interrupt = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        timer = CreateWaitableTimerExW(nullptr, nullptr, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                                       TIMER_ALL_ACCESS);
        highResolutionTimer = timer != nullptr;
        if (!timer)
            timer = CreateWaitableTimerExW(nullptr, nullptr, 0, TIMER_ALL_ACCESS);
        require(interrupt && timer, "cannot create producer timer/event");
#endif
        worker = std::thread([this] { run(); });
        std::unique_lock lock(mutex);
        changed.wait(lock, [this] { return initialized; });
    }

    ~WorkerProducer() {
        std::shared_ptr<Job> active;
        {
            std::lock_guard lock(mutex);
            active = job;
        }
        if (active)
            stop(active);
        {
            std::lock_guard lock(mutex);
            stopping = true;
        }
        changed.notify_all();
        worker.join();
#ifdef Q_OS_WIN
        CloseHandle(timer);
        CloseHandle(interrupt);
#endif
    }

    void start(const std::shared_ptr<Job>& next) {
        {
            std::lock_guard lock(mutex);
            require(!job, "producer previous capture is still active");
            job = next;
        }
        changed.notify_all();
    }

    void dispatch(const std::shared_ptr<Job>& current, qint64 now) {
        current->dispatchedNs.store(now);
#ifdef Q_OS_WIN
        SetEvent(interrupt);
#endif
    }

    void stop(const std::shared_ptr<Job>& current) {
        current->cancelled.store(true);
#ifdef Q_OS_WIN
        SetEvent(interrupt);
#endif
        std::unique_lock lock(mutex);
        changed.wait(lock, [&] { return job != current; });
    }

    bool usesHighResolutionTimer() const {
        return highResolutionTimer;
    }

  private:
    void waitUntil(qint64 deadline) {
#ifdef Q_OS_WIN
        LARGE_INTEGER due;
        due.QuadPart = -std::max<qint64>(1, (deadline - wallNanoseconds() + 99) / 100);
        SetWaitableTimerEx(timer, &due, 0, nullptr, nullptr, nullptr, 0);
        const HANDLE handles[]{interrupt, timer};
        WaitForMultipleObjects(2, handles, FALSE, INFINITE);
#else
        std::this_thread::sleep_until(
            std::chrono::steady_clock::time_point(std::chrono::nanoseconds(deadline)));
#endif
    }

    void run() {
        {
            std::lock_guard lock(mutex);
            initialized = true;
        }
        changed.notify_all();
        while (true) {
            std::shared_ptr<Job> current;
            {
                std::unique_lock lock(mutex);
                changed.wait(lock, [this] { return stopping || job != nullptr; });
                if (stopping)
                    return;
                current = job;
            }
            qint64 sequence = 0;
            qint64 previousSourceNs = 0;
            qint64 nextSourceNs = wallNanoseconds();
            bool acquired = false;
            while (!current->cancelled.load()) {
                const qint64 now = wallNanoseconds();
                const qint64 dispatch = current->dispatchedNs.load();
                const qint64 acquireDeadline = dispatch + current->acquireDelayNs;
                if (!acquired && dispatch > 0 && now >= acquireDeadline) {
                    acquired = true;
                    current->workerReadyNs.store(now);
                    QMetaObject::invokeMethod(
                        current->receiver,
                        [current] {
                            if (!current->cancelled.load())
                                current->acquired();
                        },
                        Qt::QueuedConnection);
                }
                if (now >= nextSourceNs) {
                    ++sequence;
                    {
                        std::lock_guard lock(current->samplesMutex);
                        current->sampleTimes.push_back(now);
                        if (previousSourceNs > 0)
                            current->sourceGaps.push_back(milliseconds(now - previousSourceNs));
                    }
                    current->latestSequence.store(sequence);
                    current->latestSourceNs.store(now);
                    current->input(
                        {GlobalMouseDragEvent::Kind::Update, current->gestureId,
                         settings::SettingsGlobalMouseAction::ScreenshotCopy,
                         current->initialEnd + current->step * static_cast<qreal>(sequence),
                         GlobalMouseCoordinateSpace::PhysicalPixels});
                    previousSourceNs = now;
                    nextSourceNs = now + 1000000;
                }
                qint64 deadline = nextSourceNs;
                if (!acquired && dispatch > 0)
                    deadline = std::min(deadline, acquireDeadline);
                waitUntil(deadline);
            }
            {
                std::lock_guard lock(mutex);
                job.reset();
            }
            changed.notify_all();
        }
    }

    std::mutex mutex;
    std::condition_variable changed;
    std::shared_ptr<Job> job;
    std::thread worker;
    bool initialized = false;
    bool stopping = false;
    bool highResolutionTimer = false;
#ifdef Q_OS_WIN
    HANDLE interrupt = nullptr;
    HANDLE timer = nullptr;
#endif
};

struct ProducerScope {
    WorkerProducer& producer;
    std::shared_ptr<WorkerProducer::Job> job;
    ~ProducerScope() {
        if (!job->cancelled.load())
            producer.stop(job);
    }
};

struct Paint {
    qint64 beginNs = 0;
    qint64 endNs = 0;
    qint64 cpuNs = 0;
    qint64 sampleAgeNs = 0;
    qint64 sourceAgeNs = 0;
    qint64 pendingSourceLagNs = 0;
    qint64 appliedSequence = 0;
    qint64 latestSequence = 0;
    bool moving = false;
    qsizetype display = 0;
};

struct OwnerEvent {
    qint64 ns = 0;
    int type = 0;
    bool canvas = false;
};

struct OwnerDispatch {
    qint64 beginNs = 0;
    qint64 endNs = 0;
    qint64 appliedSequence = 0;
    qint64 latestSequence = 0;
    int paintCount = 0;
};

struct InputTrace {
    qint64 ns = 0;
    qint64 sequence = 0;
    enum class Kind { Notification, Request, Commit } kind = Kind::Notification;
    bool ready = false;
};

struct Observation {
    qint64 revealNs = 0;
    qint64 firstOwnerPaintEndNs = 0;
    qint64 firstOwnerFrameDispatchEndNs = 0;
    qint64 firstMovingPaintEndNs = 0;
    qint64 firstMovingFrameDispatchEndNs = 0;
    qint64 latestAppliedSampleNs = 0;
    qint64 latestAppliedSequence = 0;
    std::atomic<qint64>* latestSourceNs = nullptr;
    std::atomic<qint64>* latestSourceSequence = nullptr;
    int ownerPaintCount = 0;
    QVector<ScreenshotOverlayWindow*> overlays;
    const ScreenshotSelectionModel* selection = nullptr;
    std::vector<Paint> paints;
    std::vector<Duration> updates;
    std::vector<Duration> surfacePreparations;
    std::vector<OwnerEvent> ownerEvents;
    std::vector<OwnerDispatch> ownerDispatches;
    QVector<int> windowActivationEvents;
    int activationEvents = 0;
    bool geometryMatchesModel = true;
};

// Observe canvas Paint and enclosing native Expose/overlay UpdateRequest dispatch, including
// Qt backing-store flush. No repaint, compositor wait, or trace write is added to this path.
class ObservedApplication final : public QApplication {
  public:
    using QApplication::QApplication;
    Observation* observation = nullptr;

    bool notify(QObject* receiver, QEvent* event) override {
        Observation* sample = observation;
        qsizetype canvasIndex = -1;
        qsizetype presentationIndex = -1;
        bool surfacePreparation = false;
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
        surfacePreparation =
            sample != nullptr &&
            event->type() ==
                ScreenshotOverlayCanvasPresenter::surfacePreparationEventTypeForTesting();
#endif
        const bool ownerAlreadyPainted = sample != nullptr && sample->firstOwnerPaintEndNs > 0;
        const bool ownerAlreadyMoved = sample != nullptr && sample->firstMovingPaintEndNs > 0;
        const int precedingOwnerPaints = sample ? sample->ownerPaintCount : 0;
        if (sample != nullptr && sample->revealNs > 0) {
            for (qsizetype index = 0; index < sample->overlays.size(); ++index) {
                auto* overlay = sample->overlays.at(index);
                if (receiver == overlay->canvas() && event->type() == QEvent::Paint)
                    canvasIndex = index;
                if ((receiver == overlay && event->type() == QEvent::UpdateRequest) ||
                    (receiver == overlay->windowHandle() && event->type() == QEvent::Expose))
                    presentationIndex = index;
                if (receiver == overlay && event->type() == QEvent::WindowActivate) {
                    ++sample->activationEvents;
                    ++sample->windowActivationEvents[index];
                }
                if (index == 0 && sample->ownerEvents.size() < 20 &&
                    (receiver == overlay || receiver == overlay->canvas()) &&
                    (event->type() == QEvent::FocusIn || event->type() == QEvent::FocusOut ||
                     event->type() == QEvent::WindowActivate))
                    sample->ownerEvents.push_back({wallNanoseconds(),
                                                   static_cast<int>(event->type()),
                                                   receiver == overlay->canvas()});
            }
        }
        if (canvasIndex < 0 && presentationIndex < 0 && !surfacePreparation)
            return QApplication::notify(receiver, event);

        const qint64 cpu = guiCpuNanoseconds();
        const qint64 begin = wallNanoseconds();
        if (canvasIndex == 0 && sample->selection != nullptr) {
            sample->geometryMatchesModel =
                sample->geometryMatchesModel && sample->overlays.at(0)->screenshotSelection() ==
                                                    sample->selection->normalizedSelection();
        }
        const qint64 sampleAge =
            sample->latestAppliedSampleNs > 0 ? begin - sample->latestAppliedSampleNs : 0;
        const qint64 sourceNs = sample->latestSourceNs ? sample->latestSourceNs->load() : 0;
        const qint64 sourceAge = sourceNs > 0 ? std::max<qint64>(0, begin - sourceNs) : 0;
        const qint64 sourceLag =
            sourceNs > 0 ? std::max<qint64>(0, sourceNs - sample->latestAppliedSampleNs) : 0;
        const qint64 appliedSequence = sample->latestAppliedSequence;
        const qint64 latestSequence =
            sample->latestSourceSequence ? sample->latestSourceSequence->load() : 0;
        const bool moving = sample->latestAppliedSampleNs > sample->revealNs;
        const bool handled = QApplication::notify(receiver, event);
        const qint64 end = wallNanoseconds();
        const qint64 cpuNs = guiCpuNanoseconds() - cpu;
        if (canvasIndex >= 0) {
            sample->paints.push_back({begin, end, cpuNs, sampleAge, sourceAge, sourceLag,
                                      appliedSequence, latestSequence, moving, canvasIndex});
            if (canvasIndex == 0)
                ++sample->ownerPaintCount;
            if (canvasIndex == 0 && sample->firstOwnerPaintEndNs == 0)
                sample->firstOwnerPaintEndNs = end;
            if (canvasIndex == 0 && moving && sample->firstMovingPaintEndNs == 0)
                sample->firstMovingPaintEndNs = end;
        }
        if (presentationIndex >= 0)
            sample->updates.push_back({end - begin, cpuNs});
        if (presentationIndex == 0 && sample->ownerPaintCount > precedingOwnerPaints &&
            sample->ownerDispatches.size() < 10)
            sample->ownerDispatches.push_back({begin, end, appliedSequence, latestSequence,
                                               sample->ownerPaintCount - precedingOwnerPaints});
        if (surfacePreparation)
            sample->surfacePreparations.push_back({end - begin, cpuNs});
        if (presentationIndex == 0 && !ownerAlreadyPainted && sample->firstOwnerPaintEndNs > 0)
            sample->firstOwnerFrameDispatchEndNs = end;
        if (presentationIndex == 0 && !ownerAlreadyMoved && sample->firstMovingPaintEndNs > 0)
            sample->firstMovingFrameDispatchEndNs = end;
        return handled;
    }
};

struct ObservationScope {
    ObservedApplication& app;
    ~ObservationScope() {
        app.observation = nullptr;
    }
};

struct FrameInputScope {
    std::function<void()>& callback;
    ~FrameInputScope() {
        callback = {};
    }
};

void settleEvents(int millisecondsToWait = 4) {
    QEventLoop loop;
    QTimer::singleShot(millisecondsToWait, &loop, &QEventLoop::quit);
    loop.exec();
}

QJsonObject statistics(std::vector<double> values) {
    if (values.empty())
        return {};
    std::sort(values.begin(), values.end());
    const auto percentile = [&values](double proportion) {
        return values.at(static_cast<std::size_t>(
            std::ceil(proportion * static_cast<double>(values.size() - 1))));
    };
    const std::size_t middle = values.size() / 2;
    const double median =
        values.size() % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2;
    return {{"median", median},
            {"p95", percentile(0.95)},
            {"max", values.back()},
            {"min", values.front()},
            {"count", static_cast<qint64>(values.size())}};
}

struct Configuration {
    QSize physicalSize;
    int overlays = 1;
    bool picker = false;
    QString key() const {
        return QStringLiteral("%1x%2-%3-overlays-picker-%4")
            .arg(physicalSize.width())
            .arg(physicalSize.height())
            .arg(overlays)
            .arg(picker ? QStringLiteral("on") : QStringLiteral("off"));
    }
};

class StartupFixture final {
  public:
    explicit StartupFixture(Configuration configuration)
        : configuration(configuration), coordinator(events, canvasRuntime, shortcuts),
          toolbar(coordinator, geometry, displays) {
        coordinator.setToolbarCommandSinks(commands, commands);
        QScreen* screen = QGuiApplication::primaryScreen();
        require(screen != nullptr, "startup benchmark requires a Qt screen");
        dpr = screen->devicePixelRatio();
        logicalSize = QSize(qRound(configuration.physicalSize.width() / dpr),
                            qRound(configuration.physicalSize.height() / dpr));
        const QRect screenRect = screen->geometry();
        const QPoint realCursor = QCursor::pos();
        const QPoint logicalOrigin(
            std::clamp(realCursor.x() - logicalSize.width() / 2, screenRect.left(),
                       screenRect.left() + std::max(0, screenRect.width() - logicalSize.width())),
            std::clamp(realCursor.y() - logicalSize.height() / 2, screenRect.top(),
                       screenRect.top() + std::max(0, screenRect.height() - logicalSize.height())));
        const QPoint physicalOrigin =
            ScreenshotGeometryMapper::physicalRectForScreen(*screen).topLeft() +
            QPoint(qRound((logicalOrigin.x() - screenRect.left()) * dpr),
                   qRound((logicalOrigin.y() - screenRect.top()) * dpr));
        precedingActivationCounts.fill(0, configuration.overlays);
        for (int index = 0; index < configuration.overlays; ++index) {
            CapturedDisplayModel display;
            display.stableId = QStringLiteral("startup-fixture-%1").arg(index);
            display.logicalRect =
                QRect(logicalOrigin + QPoint(index * logicalSize.width(), 0), logicalSize);
            display.physicalRect =
                QRect(physicalOrigin + QPoint(index * configuration.physicalSize.width(), 0),
                      configuration.physicalSize);
            display.logicalToPhysicalScale = dpr;
            display.screen = screen;
            display.active = true;
            display.geometryResolved = true;
            display.image = QImage(configuration.physicalSize, QImage::Format_RGB32);
            // Allocate/touch capture-image pixels outside all startup measurements.
            // Acquisition is deliberately controlled separately from presentation.
            display.image.fill(QColor(75 + index * 30, 120, 165));
            sourceDisplays.push_back(std::move(display));
        }
        ScreenshotPresentationServicesContext context{
            captureState, coordinator,
            toolbar,      geometry,
            displays,     interaction,
            selection,    intelligentSelection,
            {},           [this] { ++stateNotifications; }};
        context.presentSelectionDragColorPicker = [this](const QPointF& point) {
            if (picker)
                picker->updateForSelectionDrag(point, services->colorPickerContext());
        };
        services = std::make_unique<ScreenshotPresentationServices>(std::move(context));
        ScreenshotOverlayInputActions actions;
        actions.updateOverlayState = [this] { services->updateOverlayState(); };
        actions.requestSelectionDragPresentation = [this] {
            services->requestSelectionDragPresentation();
        };
        actions.hideMainToolbar = [this] { services->hideMainToolbar(); };
        actions.setOverlayCursor = [this](ScreenshotOverlayWindow* overlay,
                                          ScreenshotSelectionDragMode mode) {
            coordinator.setOverlayCursor(overlay, mode);
        };
        actions.updateColorPickerForSelectionDrag = [this](const QPointF& point) {
            if (picker)
                services->requestSelectionDragColorPickerPresentation(point);
        };
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
        actions.prepareOverlayActivation = [this] {
            if (requestActivationInput)
                requestActivationInput();
            services->flushDueSelectionDragInput();
        };
#endif
        input =
            std::make_unique<ScreenshotOverlayInputHandler>(ScreenshotOverlayInputHandlerContext{
                captureState, interaction, selection, intelligentSelection, geometry, displays,
                std::move(actions)});
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
        events.prepareActivation = [this] { input->prepareOverlayActivation(); };
#endif
        preferences.selectionTransitionAnimationEnabled = false;
        preferences.colorPickerDisplayMode = configuration.picker
                                                 ? ScreenshotColorPickerDisplayMode::AlwaysShow
                                                 : ScreenshotColorPickerDisplayMode::AlwaysHide;
        services->setUiPreferences(preferences);
        intelligentSelection.beginCaptureSession(false);
        auto backend = std::make_unique<InputBackend>();
        inputBackend = backend.get();
        mouseManager = std::make_unique<GlobalMouseManager>(std::move(backend));
        mouseManager->initialize();
        mouseManager->setCaptureAvailable(true);
    }

    ~StartupFixture() {
        mouseManager->shutdown();
        events.prepareActivation = [] {};
        requestActivationInput = {};
        input.reset();
        services.reset();
        picker.reset();
        coordinator.destroyUiResources();
        coordinator.destroyDisplayPool(displays);
        QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    }

    QJsonObject run(ObservedApplication& app, bool recreated, int acquireDelayMs, int durationMs) {
        events.activationHookNs = 0;
        events.activationHookDuration = {};
        services->resetPresentation();
        coordinator.resetForNewCapture(displays);
        selection.reset();
        interaction.beginCapture();
        captureState.sessionId++;
        captureState.sessionState = ScreenshotSessionState::Capturing;
        captureState.captureInProgress = true;
        displays.startup = std::make_shared<ScreenshotStartupContext>();
        displays.startup->sessionId = captureState.sessionId;
        displays.startup->anchorCursor = false;
        displays.startup->phase = ScreenshotStartupContext::Phase::Preparing;
        displays.startup->logicalPosition =
            sourceDisplays.front().logicalRect.topLeft() +
            QPoint(logicalSize.width() / 3, logicalSize.height() / 3);
        displays.startup->invocationLogicalPosition = displays.startup->logicalPosition;
        displays.startup->displayId = sourceDisplays.front().stableId;
        displays.startup->displaySlot = 0;
        displays.startup->layoutGeneration = 1;
        displays.startup->physicalPosition =
            sourceDisplays.front().physicalRect.topLeft() +
            QPoint(configuration.physicalSize.width() / 3, configuration.physicalSize.height() / 3);
        Observation observation;
        observation.windowActivationEvents.fill(0, configuration.overlays);
        observation.paints.reserve(256);
        observation.updates.reserve(256);
        observation.surfacePreparations.reserve(256);
        observation.ownerEvents.reserve(20);
        observation.ownerDispatches.reserve(10);
        std::vector<InputTrace> inputTrace;
        inputTrace.reserve(512);
        app.observation = &observation;
        ObservationScope observationScope{app};
        observation.selection = &selection;
        const QPointF physicalOrigin(sourceDisplays.front().physicalRect.topLeft());
        const QPointF start = physicalOrigin + QPointF(configuration.physicalSize.width() * 0.07,
                                                       configuration.physicalSize.height() * 0.1);
        const QPointF initialEnd =
            physicalOrigin + QPointF(configuration.physicalSize.width() * 0.32,
                                     configuration.physicalSize.height() * 0.4);
        QEventLoop loop;
        auto source = std::make_shared<WorkerProducer::Job>();
        source->receiver = &loop;
        source->input = inputBackend->handler;
        source->gestureId = ++gestureId;
        source->initialEnd = initialEnd;
        source->step = QPointF(configuration.physicalSize.width() * 0.25 / durationMs,
                               configuration.physicalSize.height() * 0.16 / durationMs);
        source->acquireDelayNs = static_cast<qint64>(acquireDelayMs) * 1000000;
        source->sampleTimes.reserve(4096);
        source->sourceGaps.reserve(4096);
        source->sampleTimes.front() = wallNanoseconds();
        source->latestSourceNs.store(source->sampleTimes.front());
        cursorSource = source;
        observation.latestSourceNs = &source->latestSourceNs;
        observation.latestSourceSequence = &source->latestSequence;
        input->setExternalDragActive(true);
        inputBackend->handler({GlobalMouseDragEvent::Kind::Begin, gestureId,
                               settings::SettingsGlobalMouseAction::ScreenshotCopy, start,
                               GlobalMouseCoordinateSpace::PhysicalPixels});
        inputBackend->handler({GlobalMouseDragEvent::Kind::Update, gestureId,
                               settings::SettingsGlobalMouseAction::ScreenshotCopy, initialEnd,
                               GlobalMouseCoordinateSpace::PhysicalPixels});
        // Establish the real manager gesture before the measured capture graph preparation.
        QCoreApplication::sendPostedEvents(mouseManager.get(), QEvent::MetaCall);
        const qint64 beginNs = wallNanoseconds();
        const qint64 beginCpu = guiCpuNanoseconds();
        producer.start(source);
        ProducerScope producerScope{producer, source};
        const Duration overlayPreparation = measure([&] {
            coordinator.prewarmDisplayPool(displays, configuration.overlays);
            for (qsizetype index = 0; index < displays.size(); ++index) {
                displays.displayAt(index) = sourceDisplays.at(index);
                displays.displayAt(index).image = {};
                observation.overlays.push_back(displays.overlayAt(index));
            }
            geometry.rebuild(displays);
            coordinator.prepareDisplayModels(displays);
            if (configuration.picker) {
                coordinator.createColorPicker(displays.startup->logicalPosition);
                coordinator.prepareColorPickerSurface(displays);
                picker = std::make_unique<ScreenshotColorPickerController>(
                    coordinator, geometry, displays, physicalCursor);
                picker->setDisplayMode(ScreenshotColorPickerDisplayMode::AlwaysShow);
            }
        });
        qint64 previousPulseNs = 0;
        qint64 maxPulseGapNs = 0;
        qint64 acquireReadyNs = 0;
        int deliveredSamples = 0;
        bool dragReady = false;
        int initialNotifications = 0;
        qint64 surfaceWarmsBeforeAcquire = 0;
        qint64 surfaceWarmsTotal = 0;
        QPointF bufferedPhysicalEnd = initialEnd;
        QPoint realCursorAtReveal;
        bool preferredOwnerValid = false;
        int postRevealNotifications = 0;
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
        QVector<quint64> precedingSurfaceWarmCounts;
        for (auto* overlay : observation.overlays)
            precedingSurfaceWarmCounts.push_back(
                overlay->nativeSurfacePreparationCountForTesting());
#endif
        Duration applyImages, initialSelection, postRevealSelection, postRevealLifecycle, reveal;
        bool initialSelectionValid = false;
        QTimer inputTimer;
        inputTimer.setTimerType(Qt::PreciseTimer);
        inputTimer.setInterval(1);
        QObject::connect(&inputTimer, &QTimer::timeout, &loop, [&] {
            const qint64 now = wallNanoseconds();
            if (previousPulseNs > 0)
                maxPulseGapNs = std::max(maxPulseGapNs, now - previousPulseNs);
            previousPulseNs = now;
        });
        const auto requestCurrentMovementFrame = [&] {
            if (!dragReady || source->gestureId != gestureId || source->cancelled.load())
                return;
            inputTrace.push_back({wallNanoseconds(), source->latestSequence.load(),
                                  InputTrace::Kind::Request, dragReady});
            const quint64 id = gestureId;
            services->requestSelectionDragInput([&, id] {
                if (!dragReady || id != gestureId)
                    return;
                if (const auto event = mouseManager->takePendingUpdate(id)) {
                    bufferedPhysicalEnd = event->position;
                    observation.latestAppliedSampleNs = source->sampleTime(event->position);
                    observation.latestAppliedSequence = source->sequenceFor(event->position);
                    inputTrace.push_back({wallNanoseconds(), observation.latestAppliedSequence,
                                          InputTrace::Kind::Commit, dragReady});
                    input->updateExternalSelectionDrag(
                        geometry.canvasPositionForPhysicalPoint(displays, event->position));
                    ++deliveredSamples;
                }
            });
        };
        requestActivationInput = requestCurrentMovementFrame;
        FrameInputScope frameInputScope{requestActivationInput};
        const auto movementConnection = QObject::connect(
            mouseManager.get(), &GlobalMouseManager::dragUpdateAvailable, &loop, [&](quint64 id) {
                if (id != gestureId)
                    return;
                inputTrace.push_back({wallNanoseconds(), source->latestSequence.load(),
                                      InputTrace::Kind::Notification, dragReady});
                requestCurrentMovementFrame();
            });
        const qint64 dispatchedNs = wallNanoseconds();
        source->acquired = [&] {
            acquireReadyNs = wallNanoseconds();
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
            for (qsizetype index = 0; index < observation.overlays.size(); ++index)
                surfaceWarmsBeforeAcquire += static_cast<qint64>(
                    observation.overlays.at(index)->nativeSurfacePreparationCountForTesting() -
                    precedingSurfaceWarmCounts.at(index));
#endif
            applyImages = measure([&] {
                captureState.captureInProgress = false;
                for (qsizetype index = 0; index < displays.size(); ++index)
                    displays.displayAt(index).image = sourceDisplays.at(index).image;
                coordinator.applyDisplayModels(displays);
                services->updateOverlayState();
            });
            stateNotifications = 0;
            QPointF currentEnd = initialEnd;
            qint64 initialSourceNs = source->sampleTime(initialEnd);
            if (const auto event = mouseManager->takePendingUpdate(gestureId)) {
                currentEnd = event->position;
                bufferedPhysicalEnd = currentEnd;
                initialSourceNs = source->sampleTime(currentEnd);
            }
            // The controller consumes the manager mailbox, then refreshes its endpoint from
            // the live physical cursor. Here the producer's latest sample is the synthetic cursor.
            dragReady = true;
            const auto live = source->liveSample();
            currentEnd = live.first;
            bufferedPhysicalEnd = currentEnd;
            initialSourceNs = live.second;
            observation.latestAppliedSequence = source->sequenceFor(currentEnd);
            observation.latestAppliedSampleNs = initialSourceNs;
            currentEnd = geometry.canvasPositionForPhysicalPoint(displays, currentEnd);
            const QPointF canvasStart = geometry.canvasPositionForPhysicalPoint(displays, start);
            initialSelection = measure([&] {
                input->setExternalDragActive(true);
#ifdef SNOW_SHOT_DRAG_STARTUP_BASELINE
                // These are the archived controller's exact two initialization calls.
                input->beginExternalSelectionDrag(canvasStart);
                input->updateExternalSelectionDrag(currentEnd);
#else
                input->beginExternalSelectionDrag(canvasStart, currentEnd);
#endif
            });
            initialNotifications = stateNotifications;
            initialSelectionValid =
                selection.normalizedSelection() ==
                marqueeScreenshotSelectionRect(canvasStart, geometry.canvasPositionForPhysicalPoint(
                                                                displays, bufferedPhysicalEnd));
            observation.revealNs = wallNanoseconds();
            reveal = measure([&] {
                realCursorAtReveal = QCursor::pos();
                preferredOwnerValid =
                    sourceDisplays.front().logicalRect.contains(realCursorAtReveal);
                require(preferredOwnerValid,
                        "real Qt cursor left the preferred synthetic selection display");
                coordinator.showOverlayWindows(displays,
                                               ScreenshotOverlayShowMode::CapturedImageFramePaced);
                preferredOwnerValid = preferredOwnerValid &&
                                      sourceDisplays.front().logicalRect.contains(QCursor::pos());
                require(preferredOwnerValid,
                        "real Qt cursor changed the preferred owner during reveal");
                displays.startup->phase = ScreenshotStartupContext::Phase::Revealed;
                services->discardColorPickerPresentation();
                if (picker) {
#ifdef SNOW_SHOT_DRAG_STARTUP_BASELINE
                    picker->updateAtCurrentCursor(services->colorPickerContext());
#else
                    // The external drag owns this buffered endpoint. Match the controller's
                    // restored picker request without sampling the independent live cursor.
                    services->requestSelectionDragColorPickerPresentation(
                        geometry.canvasPositionForPhysicalPoint(displays, bufferedPhysicalEnd));
#endif
                }
                const int precedingNotifications = stateNotifications;
#ifdef SNOW_SHOT_DRAG_STARTUP_BASELINE
                // The archived capturePresented callback reapplies its current controller-held
                // endpoint after the picker callback. A reentrant provider consume may update it.
                postRevealSelection = measure([&] {
                    input->updateExternalSelectionDrag(
                        geometry.canvasPositionForPhysicalPoint(displays, bufferedPhysicalEnd));
                });
#else
                // The optimized workflow publishes the Revealed lifecycle explicitly, without
                // repeating endpoint geometry mutation in the completion-only callback.
                postRevealLifecycle = measure([&] { services->updateOverlayState(); });
#endif
                postRevealNotifications = stateNotifications - precedingNotifications;
            });
            previousPulseNs = observation.revealNs;
            inputTimer.start();
            QTimer::singleShot(durationMs, &loop, &QEventLoop::quit);
        };
        producer.dispatch(source, dispatchedNs);
        const Duration surfacePreparation = measure([&] {
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
            // The optimized workflow calls this after dispatch, while acquisition runs.
            coordinator.prepareOverlaySurfaces(displays);
#endif
        });
        // The synthetic layout is already known. Mirror layout-ready presentation before pixel
        // delivery: enter the visible manual marquee mode while acquisition remains in flight.
        const Duration layoutPresentation = measure([&] {
            captureState.sessionState = ScreenshotSessionState::OverlayVisible;
            interaction.enterOverlayVisible(false);
            static_cast<void>(interaction.enterSelectionDrag(ScreenshotSelectionDragMode::Marquee));
            intelligentSelection.clearTransientState();
            services->updateOverlayState();
        });
        loop.exec();
        inputTimer.stop();
        requestActivationInput = {};
        dragReady = false;
        services->discardSelectionDragInput();
        const qint64 totalCpuNs = guiCpuNanoseconds() - beginCpu;
        app.observation = nullptr;
        QObject::disconnect(movementConnection);
        producer.stop(source);
        mouseManager->cancelGesture(gestureId);
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
        for (qsizetype index = 0; index < observation.overlays.size(); ++index)
            surfaceWarmsTotal += static_cast<qint64>(
                observation.overlays.at(index)->nativeSurfacePreparationCountForTesting() -
                precedingSurfaceWarmCounts.at(index));
#endif
        Duration surfaceWarmDispatch;
        for (const auto& duration : observation.surfacePreparations) {
            surfaceWarmDispatch.wallNs += duration.wallNs;
            surfaceWarmDispatch.cpuNs += duration.cpuNs;
        }
        require(initialSelectionValid, "startup must initialize the complete current selection");
        require(observation.geometryMatchesModel, "a painted selection was stale");
        require(deliveredSamples > 0, "the production scheduler delivered no moving samples");
        std::vector<Paint> ownerPaints;
        QVector<int> paintsPerDisplay(configuration.overlays, 0);
        std::vector<double> allPaintCpu, allPaintWall, updateWall, updateCpu, sampleAges, gaps,
            sourceAges, sourceLags, warmWalls;
        QJsonArray firstFrames;
        for (const auto& paint : observation.paints) {
            ++paintsPerDisplay[paint.display];
            allPaintCpu.push_back(milliseconds(paint.cpuNs));
            allPaintWall.push_back(milliseconds(paint.endNs - paint.beginNs));
            if (paint.display == 0)
                ownerPaints.push_back(paint);
        }
        require(ownerPaints.size() >= 3,
                "startup benchmark observed fewer than three owner paints");
        for (qsizetype index = 0; index < paintsPerDisplay.size(); ++index)
            require(paintsPerDisplay[index] > 0, "a native overlay never painted");
        for (std::size_t index = 0; index < ownerPaints.size(); ++index) {
            const auto& paint = ownerPaints[index];
            sampleAges.push_back(milliseconds(paint.sampleAgeNs));
            sourceAges.push_back(milliseconds(paint.sourceAgeNs));
            sourceLags.push_back(milliseconds(paint.pendingSourceLagNs));
            if (index > 0)
                gaps.push_back(milliseconds(paint.beginNs - ownerPaints[index - 1].beginNs));
            if (index < 10) {
                firstFrames.append(QJsonObject{
                    {"begin_after_reveal_ms", milliseconds(paint.beginNs - observation.revealNs)},
                    {"paint_wall_ms", milliseconds(paint.endNs - paint.beginNs)},
                    {"paint_gui_cpu_ms", milliseconds(paint.cpuNs)},
                    {"sample_age_ms", milliseconds(paint.sampleAgeNs)},
                    {"source_latest_age_ms", milliseconds(paint.sourceAgeNs)},
                    {"pending_source_lag_ms", milliseconds(paint.pendingSourceLagNs)},
                    {"applied_source_sequence", paint.appliedSequence},
                    {"latest_source_sequence", paint.latestSequence},
                    {"moving_sample_after_reveal", paint.moving}});
            }
        }
        for (const auto& update : observation.updates) {
            updateWall.push_back(milliseconds(update.wallNs));
            updateCpu.push_back(milliseconds(update.cpuNs));
        }
        QJsonArray perDisplay;
        for (const auto count : paintsPerDisplay)
            perDisplay.append(count);
        const auto& first = ownerPaints.front();
        require(observation.firstOwnerFrameDispatchEndNs >= first.endNs,
                "first canvas paint had no enclosing Qt presentation dispatch");
        const auto firstMoving = std::find_if(ownerPaints.begin(), ownerPaints.end(),
                                              [](const Paint& paint) { return paint.moving; });
        require(firstMoving != ownerPaints.end() &&
                    observation.firstMovingFrameDispatchEndNs >= firstMoving->endNs,
                "no moving sample had an enclosing Qt presentation dispatch");
        const auto firstMovingSource = std::upper_bound(
            source->sampleTimes.begin(), source->sampleTimes.end(), observation.revealNs);
        require(firstMovingSource != source->sampleTimes.end(),
                "source produced no revealed input");
        for (const auto& duration : observation.surfacePreparations)
            warmWalls.push_back(milliseconds(duration.wallNs));
        QJsonArray ownerEvents, ownerDispatches, inputEvents;
        for (const auto& event : observation.ownerEvents) {
            ownerEvents.append(
                QJsonObject{{"after_reveal_ms", milliseconds(event.ns - observation.revealNs)},
                            {"type", event.type == QEvent::FocusIn    ? "FocusIn"
                                     : event.type == QEvent::FocusOut ? "FocusOut"
                                                                      : "WindowActivate"},
                            {"receiver", event.canvas ? "canvas" : "window"}});
        }
        for (const auto& dispatch : observation.ownerDispatches) {
            ownerDispatches.append(QJsonObject{
                {"begin_after_reveal_ms", milliseconds(dispatch.beginNs - observation.revealNs)},
                {"end_after_reveal_ms", milliseconds(dispatch.endNs - observation.revealNs)},
                {"applied_source_sequence_at_begin", dispatch.appliedSequence},
                {"latest_source_sequence_at_begin", dispatch.latestSequence},
                {"owner_paints", dispatch.paintCount}});
        }
        for (const auto& event : inputTrace) {
            inputEvents.append(
                QJsonObject{{"after_reveal_ms", milliseconds(event.ns - observation.revealNs)},
                            {"source_sequence", event.sequence},
                            {"drag_ready", event.ready},
                            {"type", event.kind == InputTrace::Kind::Notification ? "Notification"
                                     : event.kind == InputTrace::Kind::Request    ? "Request"
                                                                                  : "Commit"}});
        }
        QJsonArray configuredDisplays;
        const auto rectangleJson = [](const QRect& rect) {
            return QJsonObject{{"x", rect.x()},
                               {"y", rect.y()},
                               {"width", rect.width()},
                               {"height", rect.height()}};
        };
        for (const auto& display : sourceDisplays)
            configuredDisplays.append(
                QJsonObject{{"logical", rectangleJson(display.logicalRect)},
                            {"physical", rectangleJson(display.physicalRect)}});
        QJsonArray activationEventsPerDisplay;
        for (int count : observation.windowActivationEvents)
            activationEventsPerDisplay.append(count);
        QJsonObject result{
            {"configuration", configuration.key()},
            {"phase", recreated ? "recreated" : "cold"},
            {"overlay_prepare", overlayPreparation.json()},
            {"surface_prepare", surfacePreparation.json()},
            {"layout_presentation", layoutPresentation.json()},
            {"surface_prepare_scope",
             "request/scheduling only; queued hidden work is timed separately"},
            {"surface_warm_dispatch", surfaceWarmDispatch.json()},
            {"surface_warm_dispatch_durations_ms", statistics(warmWalls)},
            {"surface_warm_dispatches",
             static_cast<qint64>(observation.surfacePreparations.size())},
            {"native_surface_warms_before_acquire", surfaceWarmsBeforeAcquire},
            {"native_surface_warms_total", surfaceWarmsTotal},
            {"apply_images", applyImages.json()},
            {"initial_selection", initialSelection.json()},
            {"post_reveal_selection", postRevealSelection.json()},
            {"post_reveal_lifecycle", postRevealLifecycle.json()},
            {"post_reveal_state_notifications", postRevealNotifications},
            {"reveal_call", reveal.json()},
            {"acquisition_ready_after_dispatch_ms", milliseconds(acquireReadyNs - dispatchedNs)},
            {"worker_ready_after_dispatch_ms",
             milliseconds(source->workerReadyNs.load() - dispatchedNs)},
            {"worker_ready_deadline_slack_ms",
             milliseconds(source->workerReadyNs.load() - dispatchedNs - source->acquireDelayNs)},
            {"acquisition_delivery_after_worker_ready_ms",
             milliseconds(acquireReadyNs - source->workerReadyNs.load())},
            {"startup_to_first_paint_end_ms", milliseconds(first.endNs - beginNs)},
            {"startup_to_first_frame_dispatch_end_ms",
             milliseconds(observation.firstOwnerFrameDispatchEndNs - beginNs)},
            {"reveal_to_first_frame_dispatch_end_ms",
             milliseconds(observation.firstOwnerFrameDispatchEndNs - observation.revealNs)},
            {"first_paint_end_to_frame_dispatch_end_ms",
             milliseconds(observation.firstOwnerFrameDispatchEndNs - first.endNs)},
            {"reveal_to_first_paint_begin_ms", milliseconds(first.beginNs - observation.revealNs)},
            {"reveal_to_first_paint_end_ms", milliseconds(first.endNs - observation.revealNs)},
            {"first_paint_wall_ms", milliseconds(first.endNs - first.beginNs)},
            {"first_paint_gui_cpu_ms", milliseconds(first.cpuNs)},
            {"first_moving_paint_end_after_reveal_ms",
             milliseconds(firstMoving->endNs - observation.revealNs)},
            {"first_moving_frame_dispatch_end_after_reveal_ms",
             milliseconds(observation.firstMovingFrameDispatchEndNs - observation.revealNs)},
            {"first_moving_frame_dispatch_end_after_first_source_ms",
             milliseconds(observation.firstMovingFrameDispatchEndNs - *firstMovingSource)},
            {"first_moving_paint_applied_sample_age_ms", milliseconds(firstMoving->sampleAgeNs)},
            {"first_moving_paint_source_latest_age_ms", milliseconds(firstMoving->sourceAgeNs)},
            {"first_moving_paint_pending_source_lag_ms",
             milliseconds(firstMoving->pendingSourceLagNs)},
            {"activation_hook_after_reveal_ms",
             events.activationHookNs > 0
                 ? milliseconds(events.activationHookNs - observation.revealNs)
                 : 0.0},
            {"activation_hook", events.activationHookDuration.json()},
            {"max_event_loop_pulse_gap_ms", milliseconds(maxPulseGapNs)},
            {"gui_cpu_ms", milliseconds(totalCpuNs)},
            {"initial_state_notifications", initialNotifications},
            {"window_activation_events", observation.activationEvents},
            {"window_activation_events_per_display", activationEventsPerDisplay},
            {"configured_displays", configuredDisplays},
            {"real_cursor_logical_position_at_reveal",
             QJsonObject{{"x", realCursorAtReveal.x()}, {"y", realCursorAtReveal.y()}}},
            {"preferred_reveal_owner_is_first_display", preferredOwnerValid},
            {"input_samples_submitted", source->latestSequence.load()},
            {"input_samples_delivered", deliveredSamples},
            {"input_source_interval_ms", statistics(source->sourceGaps)},
            {"producer_high_resolution_timer", producer.usesHighResolutionTimer()},
            {"canvas_paints_per_display", perDisplay},
            {"paint_wall_ms", statistics(allPaintWall)},
            {"paint_gui_cpu_ms", statistics(allPaintCpu)},
            {"native_update_wall_ms", statistics(updateWall)},
            {"native_update_gui_cpu_ms", statistics(updateCpu)},
            {"owner_paint_gap_ms", statistics(gaps)},
            {"input_sample_age_ms", statistics(sampleAges)},
            {"source_latest_age_ms", statistics(sourceAges)},
            {"pending_source_lag_ms", statistics(sourceLags)},
            {"first_owner_frames", firstFrames},
            {"first_owner_dispatches", ownerDispatches},
            {"owner_focus_activation_events", ownerEvents},
            {"input_notification_request_commit_events", inputEvents},
            {"geometry_matches_model", observation.geometryMatchesModel}};
#ifndef SNOW_SHOT_DRAG_STARTUP_BASELINE
        int activations = 0;
        QJsonArray activationsPerDisplay;
        bool onlyPreferredOwnerActivated = true;
        for (qsizetype index = 0; index < observation.overlays.size(); ++index) {
            const quint64 count =
                observation.overlays.at(index)->keyboardActivationCountForTesting();
            const int delta = static_cast<int>(count - precedingActivationCounts.at(index));
            activationsPerDisplay.append(delta);
            activations += delta;
            onlyPreferredOwnerActivated =
                onlyPreferredOwnerActivated && delta == (index == 0 ? 1 : 0);
            precedingActivationCounts[index] = count;
        }
        // Getter counters survive reuse: record delta against the preceding capture.
        result.insert("keyboard_activation_calls", activations);
        result.insert("keyboard_activation_calls_per_display", activationsPerDisplay);
        result.insert("only_preferred_keyboard_owner_activated", onlyPreferredOwnerActivated);
        require(onlyPreferredOwnerActivated,
                "keyboard activation did not target the selection owner");
#endif
        input->setExternalDragActive(false);
        services->resetPresentation();
        coordinator.hideOverlayWindows(displays);
        settleEvents();
        return result;
    }

  private:
    Configuration configuration;
    StartupOverlayEvents events;
    NoopToolbarCommands commands;
    SnowCanvasRuntime canvasRuntime;
    snow_shot::presentation::WindowShortcutManager shortcuts;
    ScreenshotOverlayCoordinator coordinator;
    ScreenshotDisplaySession displays;
    ScreenshotGeometryMapper geometry;
    ScreenshotToolbarPresenter toolbar;
    ScreenshotCaptureState captureState;
    ScreenshotInteractionState interaction;
    ScreenshotSelectionModel selection;
    ScreenshotIntelligentSelectionModel intelligentSelection;
    ScreenshotUiPreferences preferences;
    std::weak_ptr<WorkerProducer::Job> cursorSource;
    snow_shot::platform::PhysicalCursor physicalCursor{
        {true,
         [this]() -> std::optional<QPoint> {
             if (auto source = cursorSource.lock())
                 return source->liveSample().first.toPoint();
             return std::nullopt;
         },
         [](const QPoint&) { return true; }}};
    std::unique_ptr<ScreenshotColorPickerController> picker;
    std::unique_ptr<ScreenshotPresentationServices> services;
    std::unique_ptr<ScreenshotOverlayInputHandler> input;
    std::function<void()> requestActivationInput;
    InputBackend* inputBackend = nullptr;
    std::unique_ptr<GlobalMouseManager> mouseManager;
    WorkerProducer producer;
    quint64 gestureId = 0;
    QVector<CapturedDisplayModel> sourceDisplays;
    QSize logicalSize;
    qreal dpr = 1;
    int stateNotifications = 0;
    QVector<quint64> precedingActivationCounts;
};

QJsonArray summarize(const QJsonArray& runs) {
    std::map<QString, QJsonArray> groups;
    for (const auto& run : runs) {
        const auto object = run.toObject();
        const QString key = object.value("configuration").toString() + QStringLiteral("/") +
                            object.value("phase").toString();
        groups[key].append(object);
    }
    QJsonArray summary;
    for (const auto& [key, group] : groups) {
        QJsonObject object{{"case", key}};
        for (const char* metric : {"startup_to_first_paint_end_ms",
                                   "reveal_to_first_paint_end_ms",
                                   "startup_to_first_frame_dispatch_end_ms",
                                   "reveal_to_first_frame_dispatch_end_ms",
                                   "first_paint_end_to_frame_dispatch_end_ms",
                                   "first_paint_wall_ms",
                                   "worker_ready_after_dispatch_ms",
                                   "worker_ready_deadline_slack_ms",
                                   "acquisition_delivery_after_worker_ready_ms",
                                   "acquisition_ready_after_dispatch_ms",
                                   "first_moving_frame_dispatch_end_after_reveal_ms",
                                   "first_moving_frame_dispatch_end_after_first_source_ms",
                                   "first_moving_paint_applied_sample_age_ms",
                                   "first_moving_paint_source_latest_age_ms",
                                   "first_moving_paint_pending_source_lag_ms",
                                   "native_surface_warms_before_acquire",
                                   "native_surface_warms_total",
                                   "first_paint_gui_cpu_ms",
                                   "max_event_loop_pulse_gap_ms",
                                   "gui_cpu_ms",
                                   "initial_state_notifications",
                                   "window_activation_events"}) {
            std::vector<double> values;
            for (const auto& run : group)
                values.push_back(run.toObject().value(QLatin1String(metric)).toDouble());
            object.insert(QLatin1String(metric), statistics(std::move(values)));
        }
        summary.append(object);
    }
    return summary;
}
} // namespace

int main(int argc, char** argv) {
    try {
        ObservedApplication app(argc, argv);
        app.setQuitOnLastWindowClosed(false);
        QCommandLineParser parser;
        parser.addHelpOption();
        parser.addOption({"iterations", "Cold/recreated pairs per configuration.", "count", "8"});
        parser.addOption({"duration-ms", "Moving-frame observation duration.", "ms", "240"});
        parser.addOption(
            {"acquire-delay-ms", "Controlled asynchronous acquisition delay.", "ms", "20"});
        parser.addOption({"sizes", "Physical capture sizes: 1080p,4k.", "sizes", "1080p,4k"});
        parser.addOption({"overlays", "Synthetic display/window counts: 1,2.", "counts", "1,2"});
        parser.addOption({"picker", "Magnifier variants: off,on.", "variants", "off,on"});
        parser.addOption({"output", "Write JSON after all timed windows are closed.", "path"});
        parser.process(app);
        const int iterations = parser.value("iterations").toInt();
        const int duration = parser.value("duration-ms").toInt();
        const int acquireDelay = parser.value("acquire-delay-ms").toInt();
        require(iterations > 0 && iterations <= 100 && duration >= 80 && acquireDelay >= 0,
                "invalid startup benchmark iteration/duration arguments");
        IsolatedStorage storage;
        QJsonArray runs;
        for (const auto& sizeName : parser.value("sizes").split(',')) {
            require(sizeName == QStringLiteral("1080p") || sizeName == QStringLiteral("4k"),
                    "unknown physical capture size");
            const QSize physicalSize =
                sizeName == QStringLiteral("1080p") ? QSize(1920, 1080) : QSize(3840, 2160);
            for (const auto& overlayName : parser.value("overlays").split(',')) {
                const int overlayCount = overlayName.toInt();
                require(overlayCount == 1 || overlayCount == 2, "overlay count must be one or two");
                for (const auto& pickerName : parser.value("picker").split(',')) {
                    require(pickerName == QStringLiteral("off") ||
                                pickerName == QStringLiteral("on"),
                            "picker must be off or on");
                    const Configuration config{physicalSize, overlayCount,
                                               pickerName == QStringLiteral("on")};
                    for (int iteration = 0; iteration < iterations; ++iteration) {
                        StartupFixture fixture(config);
                        runs.append(fixture.run(app, false, acquireDelay, duration));
                        runs.append(fixture.run(app, true, acquireDelay, duration));
                    }
                    std::cout << config.key().toStdString() << " completed\n";
                }
            }
        }
        QJsonArray actualScreens;
        for (const auto* screen : QGuiApplication::screens()) {
            actualScreens.append(QJsonObject{{"name", screen->name()},
                                             {"width", screen->geometry().width()},
                                             {"height", screen->geometry().height()},
                                             {"dpr", screen->devicePixelRatio()},
                                             {"refresh_hz", screen->refreshRate()}});
        }
        const QJsonObject document{
            {"benchmark", "screenshot_external_drag_startup"},
#ifdef SNOW_SHOT_DRAG_STARTUP_BASELINE
            {"runtime", "baseline"},
#else
            {"runtime", "optimized"},
#endif
            {"platform", QGuiApplication::platformName()},
            {"native_windows", QGuiApplication::platformName() == QStringLiteral("windows")},
            {"input", "persistent background 1 ms source through the real GlobalMouseManager "
                      "coalescing mailbox and production frame scheduler; no physical input"},
            {"capture", "preallocated opaque image; background deadline followed by a normal "
                        "queued Qt acquisition result, independently timed"},
            {"producer_wait", "owned Windows high-resolution waitable timer when available, "
                              "otherwise an ordinary owned timer; actual source jitter and worker "
                              "deadline slack are recorded; no global timer resolution change"},
            {"display_layout", "synthetic adjacent windows on the primary Qt screen; the first "
                               "window covers the stationary real Qt cursor and the synthetic "
                               "drag. PhysicalCursor reads the producer; QCursor remains static. "
                               "Secondary windows can extend beyond the physical desktop."},
            {"measurement",
             "Qt canvas Paint and enclosing native Expose/overlay UpdateRequest dispatch including "
             "backing-store flush; no DWM presentation fence"},
            {"cpu_clock", "Windows GUI thread kernel plus user time; 100 ns tick units, accounting "
                          "may be quantized"},
            {"acquisition_delay_ms", acquireDelay},
            {"duration_ms", duration},
            {"iterations", iterations},
            {"actual_screens", actualScreens},
            {"summary", summarize(runs)},
            {"runs", runs}};
        const QByteArray json = QJsonDocument(document).toJson(QJsonDocument::Indented);
        if (parser.isSet("output")) {
            QFile output(parser.value("output"));
            require(output.open(QIODevice::WriteOnly | QIODevice::Truncate),
                    "could not open startup benchmark output");
            require(output.write(json) == json.size(), "could not write startup benchmark output");
        } else {
            std::cout << json.constData();
        }
        return 0;
    } catch (const std::exception& exception) {
        std::cerr << exception.what() << '\n';
        return 1;
    }
}
