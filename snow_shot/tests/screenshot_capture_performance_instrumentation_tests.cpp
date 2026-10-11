#include "../src/presentation/capture/screenshotcaptureperfinstrumentation.h"

#include <QCoreApplication>
#include <QElapsedTimer>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryDir>
#include <QThread>
#include <QVector>

#include <cstdlib>
#include <iostream>

namespace {
namespace perf = snow_shot::presentation::capture_perf;

void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(1);
    }
}

QVector<QJsonObject> readSamples(const QString& path) {
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    QVector<QJsonObject> samples;
    while (!file.atEnd()) {
        const QByteArray line = file.readLine();
        if (!line.endsWith('\n'))
            break;
        QJsonParseError error;
        const QJsonDocument document = QJsonDocument::fromJson(line, &error);
        require(error.error == QJsonParseError::NoError && document.isObject(),
                "Trace records must be complete JSON objects");
        samples.push_back(document.object());
    }
    return samples;
}

QVector<QJsonObject> waitForSamples(const QString& path, qsizetype count) {
    QElapsedTimer timer;
    timer.start();
    while (timer.elapsed() < 5000) {
        const auto samples = readSamples(path);
        if (samples.size() >= count)
            return samples;
        QThread::msleep(1);
    }
    require(false, "Capture trace worker must finish queued writes");
    return {};
}

void disabledTraceDoesNotWaitForComposition() {
    perf::configureTrace({});
    perf::beginSample("disabled", 1920, 1080);
    perf::milestone("disabled");
    perf::counter("disabled");
    require(!perf::flushDesktopComposition(), "Disabled tracing must not wait for DWM");
    perf::finish(true);

    QTemporaryDir directory;
    require(directory.isValid(), "Temporary trace directory must exist");
    perf::configureTrace(directory.filePath(QStringLiteral("missing/trace.jsonl")));
    perf::beginSample("invalid-path", 1920, 1080);
    require(!perf::flushDesktopComposition(), "An unavailable trace sink must not wait for DWM");
    perf::finish(true);
}

void traceWorkerPreservesOrderAndMetadata() {
    QTemporaryDir directory;
    require(directory.isValid(), "Temporary trace directory must exist");
    const QString path = directory.filePath(QStringLiteral("trace.jsonl"));
    perf::configureTrace(path);
    require(!perf::flushDesktopComposition(), "A configured sink without a sample must not wait");

    perf::beginSample("interrupted", 1920, 1080);
    perf::counter("discarded", 2);
    perf::beginSample("completed", 3840, 2160);
    {
        perf::Scope scope("paint");
        perf::counter("paint", 2);
        perf::counter("paint", 3);
        perf::milestone("presentation.reveal_queued");
    }
    perf::finish(true);
    require(!perf::flushDesktopComposition(), "A finished sample must not wait for DWM");
    perf::configureTrace({});
    const auto samples = waitForSamples(path, 2);
    require(samples.size() == 2, "Finishing must append each sample exactly once");
    require(samples.at(0).value("scenario") == QStringLiteral("interrupted") &&
                !samples.at(0).value("success").toBool(),
            "A new sample must finish its predecessor unsuccessfully and preserve order");
    const auto& completed = samples.at(1);
    require(completed.value("scenario") == QStringLiteral("completed") &&
                completed.value("success").toBool() && completed.value("width").toInt() == 3840 &&
                completed.value("height").toInt() == 2160,
            "Trace metadata must preserve the completed sample");
    require(
        completed.value("counters").toObject().value("paint").toInteger() == 5 &&
            completed.value("spans_ns").toObject().value("paint").toInteger() >= 0 &&
            completed.value("milestones_ns").toObject().contains("presentation.reveal_queued"),
        "Trace counters, scopes, and queued-reveal milestones must survive asynchronous output");
    require(!completed.value("writer_thread").toString().isEmpty() &&
                completed.value("writer_thread") != completed.value("thread"),
            "JSON serialization and file output must run away from the capture caller");
}

void reconfigurationRetainsOriginalDestinations() {
    QTemporaryDir directory;
    require(directory.isValid(), "Temporary trace directory must exist");
    const QString firstPath = directory.filePath(QStringLiteral("first.jsonl"));
    const QString secondPath = directory.filePath(QStringLiteral("second.jsonl"));
    perf::configureTrace(firstPath);
    perf::beginSample("first", 1920, 1080);
    perf::configureTrace(secondPath);
    perf::beginSample("second", 3840, 2160);
    perf::finish(true);
    perf::configureTrace({});
    perf::beginSample("disabled-after-rotation", 1, 1);
    perf::finish(true);

    const auto first = waitForSamples(firstPath, 1);
    const auto second = waitForSamples(secondPath, 1);
    require(first.size() == 1 && first.first().value("scenario") == QStringLiteral("first") &&
                !first.first().value("success").toBool(),
            "Reconfiguration must finish the old sample in its original file");
    require(second.size() == 1 && second.first().value("scenario") == QStringLiteral("second") &&
                second.first().value("success").toBool(),
            "Queued trace records must retain their destination and disabling must stop samples");
}
} // namespace

int main(int argc, char** argv) {
    QCoreApplication application(argc, argv);
    disabledTraceDoesNotWaitForComposition();
    traceWorkerPreservesOrderAndMetadata();
    reconfigurationRetainsOriginalDestinations();
    std::cout << "Capture performance instrumentation tests passed\n";
    return 0;
}
