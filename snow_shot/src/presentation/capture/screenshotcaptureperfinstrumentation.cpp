#include "screenshotcaptureperfinstrumentation.h"

#if defined(SNOW_SHOT_CAPTURE_PERF_INSTRUMENTATION)
#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMutex>
#include <QMutexLocker>
#include <QElapsedTimer>
#include <QThread>
#include <QThreadPool>

#include <utility>

#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <dwmapi.h>

namespace snow_shot::presentation::capture_perf {
namespace {
QMutex mutex;
QString tracePath;
Sink* activeSink = nullptr;
struct State {
    QElapsedTimer timer;
    QJsonObject spans;
    QJsonObject milestones;
    QJsonObject counters;
    QString scenario;
    qint64 width = 0;
    qint64 height = 0;
    bool active = false;
};
State state;

class TraceWriter final {
  public:
    TraceWriter() {
        m_pool.setMaxThreadCount(1);
        m_pool.setExpiryTimeout(-1);
        // Configuration happens before captures. Start the dedicated writer now
        // so the first trace finish does not also launch a thread during reveal.
        m_pool.start([] {});
    }

    void append(QString path, QJsonObject sample) {
        // Serialize and append on one worker, preserving sample order without
        // putting disk I/O on the capture or first moving-frame critical path.
        m_pool.start([path = std::move(path), sample = std::move(sample)]() mutable {
            sample[QStringLiteral("writer_thread")] =
                QString::number(reinterpret_cast<quintptr>(QThread::currentThreadId()));
            QFile file(path);
            if (!file.open(QIODevice::WriteOnly | QIODevice::Append)) {
                qWarning("Unable to open capture performance trace: %s",
                         qPrintable(file.errorString()));
                return;
            }
            QByteArray record = QJsonDocument(sample).toJson(QJsonDocument::Compact);
            record.append('\n');
            if (file.write(record) != record.size())
                qWarning("Unable to write capture performance trace: %s",
                         qPrintable(file.errorString()));
        });
    }

  private:
    // QThreadPool drains queued writes at destruction, including application exit.
    QThreadPool m_pool;
};

TraceWriter& traceWriter() {
    static TraceWriter writer;
    return writer;
}

class FileSink final : public Sink {
  public:
    void recordScope(const char* name, qint64 nanoseconds) override {
        if (state.active) {
            const QString key = QString::fromLatin1(name);
            state.spans[key] = state.spans.value(key).toInteger() + nanoseconds;
        }
    }
    void recordMilestone(const char* name, qint64 nanoseconds) override {
        if (state.active)
            state.milestones[QString::fromLatin1(name)] = nanoseconds;
    }
    void recordCounter(const char* name, qint64 value) override {
        if (state.active) {
            const QString key = QString::fromLatin1(name);
            state.counters[key] = state.counters.value(key).toInteger() + value;
        }
    }
    void finish(bool success) override {
        if (!state.active)
            return;
        QJsonObject object{{QStringLiteral("timestamp_utc"),
                            QDateTime::currentDateTimeUtc().toString(Qt::ISODateWithMs)},
                           {QStringLiteral("scenario"), state.scenario},
                           {QStringLiteral("width"), state.width},
                           {QStringLiteral("height"), state.height},
                           {QStringLiteral("success"), success},
                           {QStringLiteral("end_to_end_ns"), state.timer.nsecsElapsed()},
                           {QStringLiteral("spans_ns"), state.spans},
                           {QStringLiteral("milestones_ns"), state.milestones},
                           {QStringLiteral("counters"), state.counters},
                           {QStringLiteral("thread"), QString::number(reinterpret_cast<quintptr>(
                                                          QThread::currentThreadId()))}};
        state.active = false;
        traceWriter().append(tracePath, std::move(object));
    }
};
FileSink fileSink;
} // namespace

void configureTrace(const QString& path) {
    QMutexLocker lock(&mutex);
    if (state.active)
        fileSink.finish(false);
    tracePath.clear();
    if (!path.isEmpty()) {
        QFile file(path);
        if (file.open(QIODevice::WriteOnly | QIODevice::Append))
            tracePath = QFileInfo(file).absoluteFilePath();
    }
    activeSink = tracePath.isEmpty() ? nullptr : &fileSink;
    if (activeSink != nullptr)
        static_cast<void>(traceWriter());
}
void beginSample(const char* scenario, qint64 width, qint64 height) {
    QMutexLocker lock(&mutex);
    if (activeSink == nullptr)
        return;
    if (state.active)
        fileSink.finish(false);
    state = {};
    state.scenario = QString::fromLatin1(scenario);
    state.width = width;
    state.height = height;
    state.timer.start();
    state.active = true;
}
void milestone(const char* name) {
    QMutexLocker lock(&mutex);
    if (activeSink && state.active) {
        const qint64 elapsed = state.timer.nsecsElapsed();
        activeSink->recordMilestone(name, elapsed);
    }
}
void counter(const char* name, qint64 value) {
    QMutexLocker lock(&mutex);
    if (activeSink && state.active)
        activeSink->recordCounter(name, value);
}
bool flushDesktopComposition() {
    {
        QMutexLocker lock(&mutex);
        if (activeSink == nullptr || !state.active)
            return false;
    }
    static_cast<void>(DwmFlush());
    return true;
}
Scope::~Scope() {
    const qint64 elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
                               std::chrono::steady_clock::now() - m_started)
                               .count();
    QMutexLocker lock(&mutex);
    if (activeSink && state.active)
        activeSink->recordScope(m_name, elapsed);
}
void finish(bool success) {
    QMutexLocker lock(&mutex);
    if (activeSink && state.active)
        activeSink->finish(success);
}
} // namespace snow_shot::presentation::capture_perf
#else
namespace snow_shot::presentation::capture_perf {
void configureTrace(const QString&) {}
void beginSample(const char*, qint64, qint64) {}
void milestone(const char*) {}
void counter(const char*, qint64) {}
bool flushDesktopComposition() {
    return false;
}
Scope::~Scope() = default;
void finish(bool) {}
} // namespace snow_shot::presentation::capture_perf
#endif
