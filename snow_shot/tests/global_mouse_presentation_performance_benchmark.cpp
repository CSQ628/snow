#include "screenshot_selection_presentation_fixture.h"
#include "snow_shot/presentation/globalmousemanager.h"
#include "snow_shot/presentation/screenshotglobalmousedrag.h"
#include "snow_shot/presentation/screenshotoverlayinputhandler.h"

#include <QChronoTimer>
#include <QCommandLineParser>
#include <QElapsedTimer>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTextStream>
#include <QTimer>
#include <qpa/qwindowsysteminterface.h>

#include <algorithm>
#include <array>
#include <iostream>
#include <limits>
#include <numeric>
#include <vector>

namespace {
using namespace snow_shot::presentation;
using selection_presentation_test::Fixture;
using selection_presentation_test::FrameBackend;
using Kind = GlobalMouseDragEvent::Kind;
constexpr qint64 kOriginNs = 1000000000;
constexpr qint64 kNever = std::numeric_limits<qint64>::max();
#ifdef SNOW_SHOT_GLOBAL_MOUSE_PRESENTATION_BASELINE
constexpr bool kBaseline = true;
#else
constexpr bool kBaseline = false;
#endif

void require(bool condition, const char* message) {
    if (!condition)
        throw std::runtime_error(message);
}

QJsonObject distribution(std::vector<double> values) {
    if (values.empty())
        return {{"count", 0}, {"mean", 0}, {"p50", 0}, {"p95", 0}, {"p99", 0}, {"max", 0}};
    const double mean =
        std::accumulate(values.begin(), values.end(), 0.0) / static_cast<double>(values.size());
    std::sort(values.begin(), values.end());
    const auto percentile = [&](double fraction) {
        return values[static_cast<std::size_t>(
            std::ceil(fraction * static_cast<double>(values.size())) - 1)];
    };
    return {{"count", static_cast<qint64>(values.size())},
            {"mean", mean},
            {"p50", percentile(0.50)},
            {"p95", percentile(0.95)},
            {"p99", percentile(0.99)},
            {"max", values.back()}};
}

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

// Dispatch the actual production timer objects on a virtual timeline. Wall-clock event-loop
// timers are never pumped during replay. Their requested intervals, callbacks and rearming
// behavior remain production code; these are cadence replay results, not native timer latency.
class TimerReplay final {
  public:
    explicit TimerReplay(QObject* object = nullptr, bool jitter = false)
        : m_object(object), m_jitter(jitter) {}

    void sync(qint64 now) {
        Qt::TimerId id = Qt::TimerId::Invalid;
        qint64 interval = 0;
        if (auto* chronoTimer = qobject_cast<QChronoTimer*>(m_object)) {
            if (chronoTimer->isActive()) {
                id = chronoTimer->id();
                interval = chronoTimer->interval().count();
            }
        } else if (auto* millisecondTimer = qobject_cast<QTimer*>(m_object)) {
            if (millisecondTimer->isActive()) {
                id = millisecondTimer->id();
                interval = static_cast<qint64>(millisecondTimer->interval()) * 1000000;
            }
        }
        if (id == Qt::TimerId::Invalid) {
            m_id = id;
            wakeNs = kNever;
        } else if (id != m_id) {
            m_id = id;
            constexpr std::array<qint64, 4> delays{0, 220000, 0, 850000};
            wakeNs = now + interval + (m_jitter ? delays[m_arms++ % delays.size()] : 0);
        }
    }

    void dispatch(qint64 now) {
        require(m_object && m_id != Qt::TimerId::Invalid && now >= wakeNs,
                "timer replay must dispatch an active due timer");
        QTimerEvent event(m_id);
        QCoreApplication::sendEvent(m_object, &event);
        ++wakeups;
        sync(now);
    }

    qint64 wakeNs = kNever;
    qint64 wakeups = 0;

  private:
    QObject* m_object;
    bool m_jitter;
    Qt::TimerId m_id = Qt::TimerId::Invalid;
    std::size_t m_arms = 0;
};

struct Case {
    qreal refreshRate;
    int inputRate;
    QString scenario;
};

class Replay final {
  public:
    Replay(Case benchmarkCase, QSize size, int durationMs, int warmupMs, bool magnifier)
        : m_case(std::move(benchmarkCase)), m_fixture(size, false, true, FrameBackend::QtTimer),
          m_durationNs(static_cast<qint64>(durationMs) * 1000000),
          m_warmupNs(static_cast<qint64>(warmupMs) * 1000000),
          m_inputPeriodNs(qRound64(1000000000.0 / m_case.inputRate)),
          m_refreshRate(m_case.refreshRate), m_magnifier(magnifier),
          m_presentationTimer(m_fixture.services->frameSchedulerForTesting().wakeupObject(),
                              m_case.scenario == QStringLiteral("jitter")) {
        auto backend = std::make_unique<InputBackend>();
        m_backend = backend.get();
        m_manager = std::make_unique<GlobalMouseManager>(std::move(backend));
        m_manager->initialize();
#ifdef SNOW_SHOT_GLOBAL_MOUSE_PRESENTATION_BASELINE
        m_managerTimer =
            TimerReplay(m_manager->findChild<QTimer*>(QStringLiteral("globalMouseFrameTimer")),
                        m_case.scenario == QStringLiteral("jitter"));
#endif
        ScreenshotOverlayInputActions actions;
        actions.updateOverlayState = [this] { m_fixture.services->updateOverlayState(); };
        actions.requestSelectionDragPresentation = [this] {
            m_fixture.services->requestSelectionDragPresentation();
        };
        if (magnifier) {
            m_fixture.enableColorPicker(ScreenshotColorPickerDisplayMode::AlwaysShow);
            actions.updateColorPickerForSelectionDrag = [this](const QPointF& position) {
                m_fixture.services->requestSelectionDragColorPickerPresentation(position);
            };
        }
        m_input =
            std::make_unique<ScreenshotOverlayInputHandler>(ScreenshotOverlayInputHandlerContext{
                m_fixture.captureState, m_fixture.interaction, m_fixture.selection,
                m_fixture.intelligentSelection, m_fixture.geometry, m_fixture.displays,
                std::move(actions)});
        // The notification observes committed renderer state while still inside the real
        // presentation call, so two synchronous geometry commits cannot hide behind one paint.
        m_fixture.onStateChanged = [this] { observe(m_dispatchingPresentation, m_followsStall); };
        QObject::connect(m_manager.get(), &GlobalMouseManager::dragEvent, m_manager.get(),
                         [this](const auto& event) { boundaryOrMovement(event); });
#ifndef SNOW_SHOT_GLOBAL_MOUSE_PRESENTATION_BASELINE
        QObject::connect(m_manager.get(), &GlobalMouseManager::dragUpdateAvailable, m_manager.get(),
                         [this](quint64 id) {
                             if (m_measuring)
                                 ++m_updateNotifications;
                             m_fixture.services->requestSelectionDragInput([this, id] {
                                 if (const auto event = m_manager->takePendingUpdate(id))
                                     applyMovement(*event);
                             });
                         });
#endif
        setRefreshRate(m_refreshRate);
        m_samples.push_back(kOriginNs);
        post(Kind::Begin, QPointF(20, 20));
        drain();
        paint();
        m_previous = m_fixture.displayedSelection();
        syncTimers();
    }

    ~Replay() {
        m_fixture.onStateChanged = {};
        m_fixture.services->discardSelectionDragInput();
        m_manager->shutdown();
    }

    QJsonObject run() {
        const qint64 endNs = kOriginNs + m_warmupNs + m_durationNs;
        const qint64 stallStart = kOriginNs + m_warmupNs + m_durationNs / 3;
        const qint64 stallEnd = stallStart + qRound64(3000000000.0 / m_case.refreshRate);
        const qint64 idleStart = kOriginNs + m_warmupNs + m_durationNs / 3;
        const qint64 idleEnd = idleStart + m_durationNs / 4;
        qint64 nextInput = kOriginNs + m_inputPeriodNs;
        qint64 nextRefresh = m_case.scenario == QStringLiteral("mixed")
                                 ? kOriginNs + m_warmupNs + m_durationNs / 3
                                 : kNever;
        int refreshChanges = 0;
        while (true) {
            if (m_case.scenario == QStringLiteral("idle") && nextInput >= idleStart &&
                nextInput < idleEnd)
                nextInput = idleEnd;
            const auto unblocked = [&](qint64 wake) {
                return m_case.scenario == QStringLiteral("stall") && wake >= stallStart &&
                               wake < stallEnd
                           ? stallEnd
                           : wake;
            };
            const qint64 managerWake = unblocked(m_managerTimer.wakeNs);
            const qint64 presentationWake = unblocked(m_presentationTimer.wakeNs);
            const qint64 next = std::min({nextInput, nextRefresh, managerWake, presentationWake});
            if (next >= endNs)
                break;
            m_fixture.nowNanoseconds = next;
            m_measuring = next >= kOriginNs + m_warmupNs;
            if (next == nextRefresh) {
                setRefreshRate(refreshChanges++ == 0 ? 240.0 : 60.0);
                nextRefresh =
                    refreshChanges == 1 ? kOriginNs + m_warmupNs + m_durationNs * 2 / 3 : kNever;
            }
            if (next == nextInput) {
                ++m_latestSequence;
                m_samples.push_back(next);
                timed(m_submissionWork, [&] { post(Kind::Update, point(m_latestSequence)); });
                constexpr std::array<int, 4> jitter{0, 1, 0, -1};
                nextInput += m_inputPeriodNs +
                             (m_case.scenario == QStringLiteral("jitter")
                                  ? jitter[static_cast<std::size_t>(m_latestSequence) % 4] *
                                        m_inputPeriodNs / 4
                                  : 0);
            }
            const bool stalled =
                m_case.scenario == QStringLiteral("stall") && next >= stallStart && next < stallEnd;
            if (stalled)
                continue;
            drain();
            observe(false, false);
            syncTimers();
            if (m_managerTimer.wakeNs <= next) {
                timed(m_dispatchWork, [&] { m_managerTimer.dispatch(next); });
                if (m_measuring)
                    ++m_managerWakeups;
                observe(false, false);
                syncTimers();
            }
            if (m_presentationTimer.wakeNs <= next) {
                m_dispatchingPresentation = true;
                m_followsStall = next >= stallEnd && m_case.scenario == QStringLiteral("stall") &&
                                 next - stallEnd < qRound64(1000000000.0 / m_refreshRate);
                timed(m_frameWork, [&] { m_presentationTimer.dispatch(next); });
                if (m_measuring)
                    ++m_presentationWakeups;
                observe(true, m_followsStall);
                m_dispatchingPresentation = false;
                m_followsStall = false;
            }
            paint();
            syncTimers();
        }
        m_fixture.nowNanoseconds = endNs;
        m_measuring = false;
        // An exact release supersedes a movement that has not reached its presentation frame.
        post(Kind::Update, point(++m_latestSequence));
        const QPointF release = point(m_latestSequence + 1);
        post(Kind::Finish, release);
        drain();
        paint();
        require(m_fixture.displayedSelection() ==
                        marqueeScreenshotSelectionRect(QPointF(20.0, 20.0), release) &&
                    m_finishes == 1,
                "release must synchronously commit its exact pixel-cell selection once");
        const QRectF finished = m_fixture.displayedSelection();
        post(Kind::Update, point(m_latestSequence + 2));
        drain();
        m_fixture.nowNanoseconds += 100000000;
        syncTimers();
        require(m_fixture.displayedSelection() == finished &&
                    m_presentationTimer.wakeNs == kNever && m_managerTimer.wakeNs == kNever,
                "ended gestures must retire both movement and presentation deadlines");
        verifyCancellation();
        require(m_geometryCommits > 0 && m_canvasPaints > 0,
                "the measured replay must commit and paint real selection geometry");
        if (!kBaseline) {
            require(m_staleCommits == 0, "shared frames must sample the latest producer point");
            require(m_directCommits == 0, "continuous updates must commit inside the shared frame");
            require(m_duplicateCommits == 0, "a shared frame must not commit geometry twice");
            require(m_maxPaintsPerDispatch <= 1, "one frame must paint the canvas at most once");
            if (m_case.scenario == QStringLiteral("steady")) {
                const qint64 periodNs = qRound64(1000000000.0 / m_case.refreshRate);
                const qint64 expectedCommits = m_durationNs / periodNs;
                require(qAbs(m_geometryCommits - expectedCommits) <= 1,
                        "steady replay must commit at the selected screen refresh cadence");
                require(std::all_of(m_geometryGaps.begin(), m_geometryGaps.end(),
                                    [periodNs](double gapMs) {
                                        return qAbs(qRound64(gapMs * 1000000.0) - periodNs) <= 1;
                                    }),
                        "steady geometry gaps must equal one rounded screen period within 1 ns");
                require(std::all_of(m_sampleAge.begin(), m_sampleAge.end(),
                                    [this](double ageMs) {
                                        const qint64 ageNs = qRound64(ageMs * 1000000.0);
                                        return ageNs >= 0 && ageNs <= m_inputPeriodNs + 1;
                                    }),
                        "steady frames must sample input no older than one input period");
            }
        }
        const qint64 canvasPaints = m_canvasPaints;
        const double totalWork =
            sum(m_submissionWork) + sum(m_dispatchWork) + sum(m_frameWork) + sum(m_paintWork);
        return {{"scenario", m_case.scenario},
                {"initial_refresh_hz", m_case.refreshRate},
                {"input_hz", m_case.inputRate},
                {"magnifier_included", m_magnifier},
                {"virtual_duration_ms", static_cast<double>(m_durationNs) / 1000000.0},
                {"input_samples", m_measuredInputs},
                {"geometry_commits", m_geometryCommits},
                {"direct_geometry_commits", m_directCommits},
                {"canvas_paints", canvasPaints},
                {"max_canvas_paints_per_dispatch", m_maxPaintsPerDispatch},
                {"stale_geometry_commits", m_staleCommits},
                {"duplicate_geometry_commits", m_duplicateCommits},
                {"presentation_timer_wakeups", m_presentationWakeups},
                {"manager_timer_wakeups", m_managerWakeups},
                {"movement_notifications", m_updateNotifications},
                {"virtual_sample_age_ms", distribution(std::move(m_sampleAge))},
                {"virtual_geometry_gap_ms", distribution(std::move(m_geometryGaps))},
                {"virtual_unstalled_geometry_gap_ms", distribution(std::move(m_unstalledGaps))},
                {"submission_work_ms", distribution(std::move(m_submissionWork))},
                {"manager_dispatch_work_ms", distribution(std::move(m_dispatchWork))},
                {"presentation_frame_work_ms", distribution(std::move(m_frameWork))},
                {"paint_dispatch_work_ms", distribution(std::move(m_paintWork))},
                {"total_measured_active_work_ms", totalWork},
                {"work_ms_per_geometry_commit",
                 m_geometryCommits > 0
                     ? QJsonValue(totalWork / static_cast<double>(m_geometryCommits))
                     : QJsonValue()},
                {"work_ms_per_canvas_paint",
                 canvasPaints > 0 ? QJsonValue(totalWork / static_cast<double>(canvasPaints))
                                  : QJsonValue()},
                {"exact_release_verified", true},
                {"cancellation_verified", true}};
    }

  private:
    static double sum(const std::vector<double>& values) {
        return std::accumulate(values.begin(), values.end(), 0.0);
    }
    template <typename Callback> void timed(std::vector<double>& samples, Callback callback) {
        QElapsedTimer timer;
        timer.start();
        callback();
        if (m_measuring)
            samples.push_back(static_cast<double>(timer.nsecsElapsed()) / 1000000.0);
    }
    static QPointF point(qint64 sequence) {
        return {80.0 + static_cast<qreal>(sequence % 800),
                80.0 + static_cast<qreal>(sequence / 800)};
    }
    static qint64 sequence(const QPointF& position) {
        return qRound64(position.x() - 80.0) + 800 * qRound64(position.y() - 80.0);
    }
    void setRefreshRate(qreal rate) {
        m_refreshRate = rate;
        QScreen* screen = m_fixture.overlay.screen();
        QWindowSystemInterface::handleScreenRefreshRateChange(screen, rate);
        QWindowSystemInterface::sendWindowSystemEvents(QEventLoop::AllEvents);
        require(qFuzzyCompare(screen->refreshRate(), rate), "replay refresh rate must be applied");
    }
    void post(Kind kind, const QPointF& position) {
        m_backend->handler({kind, m_gestureId, settings::SettingsGlobalMouseAction::ScreenshotCopy,
                            position, GlobalMouseCoordinateSpace::DesktopPoints});
        if (m_measuring && kind == Kind::Update)
            ++m_measuredInputs;
    }
    void drain() {
        timed(m_dispatchWork,
              [&] { QCoreApplication::sendPostedEvents(m_manager.get(), QEvent::MetaCall); });
    }
    void paint() {
        const qint64 before = m_fixture.paintObserver.canvasPaints;
        timed(m_paintWork,
              [] { QCoreApplication::sendPostedEvents(nullptr, QEvent::UpdateRequest); });
        if (m_measuring) {
            const qint64 painted = m_fixture.paintObserver.canvasPaints - before;
            m_canvasPaints += painted;
            m_maxPaintsPerDispatch = std::max(m_maxPaintsPerDispatch, painted);
        }
        // Diagnostic unions are used by other rendering benchmarks; retaining a history of
        // damage here would measure accumulation work absent from normal production builds.
        resetSelectionRenderDiagnosticsForCurrentThread();
        resetGuideLineRenderDiagnosticsForCurrentThread();
    }
    void syncTimers() {
        m_presentationTimer.sync(m_fixture.nowNanoseconds);
        m_managerTimer.sync(m_fixture.nowNanoseconds);
    }
    void applyMovement(const GlobalMouseDragEvent& event) {
        if (m_drag.update(event.id, event.position, event.kind == Kind::Finish))
            m_input->updateExternalSelectionDrag(m_drag.end());
    }
    void boundaryOrMovement(const GlobalMouseDragEvent& event) {
        if (event.kind == Kind::Begin) {
            m_fixture.services->discardSelectionDragInput();
            m_fixture.selection.clearSelection();
            m_fixture.interaction.returnToSelectionMode(false);
            m_fixture.services->updateOverlayState();
            m_input->setExternalDragActive(true);
            m_drag.begin(event.id, event.position, event.coordinateSpace);
            m_drag.setReady();
            m_input->beginExternalSelectionDrag(event.position);
        } else if (event.kind == Kind::Update) {
            if (m_measuring)
                ++m_updateNotifications;
            applyMovement(event);
        } else if (event.kind == Kind::Finish) {
            m_fixture.services->discardSelectionDragInput();
            applyMovement(event);
            m_fixture.interaction.finishDrag();
            m_input->setExternalDragActive(false);
            m_drag.reset();
            m_input->confirmSelection();
            m_manager->cancelGesture(event.id);
            ++m_finishes;
        } else {
            m_fixture.services->discardSelectionDragInput();
            m_fixture.services->resetPresentation();
            m_fixture.interaction.cancelDrag();
            m_input->setExternalDragActive(false);
            m_drag.reset();
            m_manager->cancelGesture(event.id);
        }
    }
    void observe(bool sharedFrame, bool followsStall) {
        const QRectF geometry = m_fixture.displayedSelection();
        if (geometry == m_previous)
            return;
        m_previous = geometry;
        if (!m_measuring || !geometry.isValid())
            return;
        const qint64 presentedSequence = sequence(geometry.bottomRight() - QPointF(1.0, 1.0));
        require(presentedSequence > 0 && presentedSequence < static_cast<qint64>(m_samples.size()),
                "displayed geometry must resolve to an actual input sample");
        ++m_geometryCommits;
        if (!sharedFrame)
            ++m_directCommits;
        if (presentedSequence != m_latestSequence)
            ++m_staleCommits;
        m_sampleAge.push_back(
            static_cast<double>(m_fixture.nowNanoseconds -
                                m_samples[static_cast<std::size_t>(presentedSequence)]) /
            1000000.0);
        if (m_lastCommitNs != 0) {
            const double gap =
                static_cast<double>(m_fixture.nowNanoseconds - m_lastCommitNs) / 1000000.0;
            m_geometryGaps.push_back(gap);
            if (!followsStall && m_case.scenario != QStringLiteral("idle"))
                m_unstalledGaps.push_back(gap);
            if (m_lastCommitNs == m_fixture.nowNanoseconds)
                ++m_duplicateCommits;
        }
        m_lastCommitNs = m_fixture.nowNanoseconds;
    }
    void verifyCancellation() {
        ++m_gestureId;
        post(Kind::Begin, QPointF(20, 20));
        drain();
        post(Kind::Update, QPointF(100, 100));
        drain();
        post(Kind::Cancel, {});
        drain();
        const QRectF canceled = m_fixture.displayedSelection();
        post(Kind::Update, QPointF(200, 200));
        drain();
        m_fixture.nowNanoseconds += 100000000;
        syncTimers();
        paint();
        require(m_fixture.displayedSelection() == canceled &&
                    m_presentationTimer.wakeNs == kNever && m_managerTimer.wakeNs == kNever,
                "cancellation must discard deferred movement and frame callbacks");
    }

    Case m_case;
    Fixture m_fixture;
    InputBackend* m_backend = nullptr;
    std::unique_ptr<GlobalMouseManager> m_manager;
    std::unique_ptr<ScreenshotOverlayInputHandler> m_input;
    ScreenshotGlobalMouseDrag m_drag;
    qint64 m_durationNs;
    qint64 m_warmupNs;
    qint64 m_inputPeriodNs;
    qreal m_refreshRate;
    bool m_magnifier;
    bool m_measuring = false;
    bool m_dispatchingPresentation = false;
    bool m_followsStall = false;
    TimerReplay m_presentationTimer;
    TimerReplay m_managerTimer;
    QRectF m_previous;
    std::vector<qint64> m_samples;
    std::vector<double> m_submissionWork, m_dispatchWork, m_frameWork, m_paintWork;
    std::vector<double> m_sampleAge, m_geometryGaps, m_unstalledGaps;
    quint64 m_gestureId = 1;
    qint64 m_latestSequence = 0, m_measuredInputs = 0, m_geometryCommits = 0;
    qint64 m_directCommits = 0, m_staleCommits = 0, m_duplicateCommits = 0;
    qint64 m_canvasPaints = 0, m_maxPaintsPerDispatch = 0, m_lastCommitNs = 0;
    qint64 m_presentationWakeups = 0, m_managerWakeups = 0;
    qint64 m_updateNotifications = 0;
    int m_finishes = 0;
};

std::vector<int> numbers(const QString& text, int minimum, int maximum) {
    std::vector<int> result;
    for (const QString& part : text.split(',')) {
        bool valid = false;
        const int number = part.toInt(&valid);
        require(valid && number >= minimum && number <= maximum, "invalid benchmark number");
        result.push_back(number);
    }
    require(!result.empty(), "benchmark list must not be empty");
    return result;
}
} // namespace

static int runBenchmark(int argc, char* argv[]) {
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    application.setQuitOnLastWindowClosed(false);
    QCommandLineParser parser;
    parser.setApplicationDescription(
        QStringLiteral("Global mouse to screenshot presentation replay"));
    parser.addHelpOption();
    parser.addOption({"rates", "Refresh rates in Hz", "list", "120,144,165,240"});
    parser.addOption({"input-rates", "Input rates in Hz", "list", "1000,8000"});
    parser.addOption({"scenario", "all, steady, jitter, stall, idle, or mixed", "name", "all"});
    parser.addOption({"duration-ms", "Measured virtual duration", "milliseconds", "360"});
    parser.addOption({"warmup-ms", "Virtual warmup duration", "milliseconds", "60"});
    parser.addOption({"magnifier", "Include the real selection drag magnifier"});
    parser.addOption({"output", "JSON report path", "path"});
    parser.process(application);
    const auto rates = numbers(parser.value("rates"), 30, 360);
    const auto inputRates = numbers(parser.value("input-rates"), 500, 8000);
    const int duration = numbers(parser.value("duration-ms"), 100, 5000).front();
    const int warmup = numbers(parser.value("warmup-ms"), 10, 1000).front();
    const QString selected = parser.value("scenario");
    const std::array scenarios{QStringLiteral("steady"), QStringLiteral("jitter"),
                               QStringLiteral("stall"), QStringLiteral("idle"),
                               QStringLiteral("mixed")};
    require(selected == QStringLiteral("all") ||
                std::find(scenarios.begin(), scenarios.end(), selected) != scenarios.end(),
            "unknown benchmark scenario");
    selection_presentation_test::IsolatedStorage storage;
    QScreen* screen = QGuiApplication::primaryScreen();
    const qreal originalRate = screen->refreshRate();
    QJsonArray results;
    for (int rate : rates)
        for (int inputRate : inputRates)
            for (const QString& scenario : scenarios)
                if (selected == QStringLiteral("all") || selected == scenario) {
                    try {
                        results.append(Replay({static_cast<qreal>(rate), inputRate, scenario},
                                              QSize(960, 640), duration, warmup,
                                              parser.isSet("magnifier"))
                                           .run());
                    } catch (const std::exception& error) {
                        throw std::runtime_error(
                            QStringLiteral("%1 at %2 Hz refresh / %3 Hz input: %4")
                                .arg(scenario)
                                .arg(rate)
                                .arg(inputRate)
                                .arg(QString::fromUtf8(error.what()))
                                .toStdString());
                    }
                }
    QWindowSystemInterface::handleScreenRefreshRateChange(screen, originalRate);
    QWindowSystemInterface::sendWindowSystemEvents(QEventLoop::AllEvents);
    const QJsonObject report{
        {"schema_version", 1},
        {"benchmark", "global_mouse_presentation"},
        {"implementation", kBaseline ? "archived_rounded_manager_current_ui" : "shared_frame"},
        {"platform", QGuiApplication::platformName()},
        {"qt", qVersion()},
#ifdef NDEBUG
        {"build", "release"},
#else
        {"build", "debug"},
#endif
        {"measurement",
         "Cadence and sample age use deterministic virtual input and timer-event "
         "replay through the real manager, external selection handler, presentation "
         "services and canvas. Work distributions measure wall time of active "
         "calls, including replay dispatch and commit-observation overhead, "
         "excluding fixture creation and virtual waiting; they are not CPU time. The optional "
         "baseline links archived production manager sources to identical current "
         "UI code. Its real QDeadlineTimer chooses intervals using measured wall "
         "subscriber work; those requested intervals are replayed virtually. "
         "Refresh changes affect this process's QScreen only and model mixed-rate "
         "screen transitions without testing coordinate mapping. Physical hook "
         "delivery, OS timer jitter, compositor completion and input-to-photon "
         "latency are excluded. Compare work per commit/paint together with counts."},
        {"results", results}};
    const QByteArray json = QJsonDocument(report).toJson(QJsonDocument::Indented);
    if (parser.isSet("output")) {
        QFile file(parser.value("output"));
        require(file.open(QIODevice::WriteOnly | QIODevice::Truncate),
                "cannot save benchmark JSON");
        require(file.write(json) == json.size(), "cannot write complete benchmark JSON");
    } else {
        QTextStream(stdout) << json;
    }
    return 0;
}

int main(int argc, char* argv[]) {
    try {
        return runBenchmark(argc, argv);
    } catch (const std::exception& error) {
        std::cerr << "Global mouse presentation benchmark failed: " << error.what() << '\n';
        return 1;
    }
}
