#include "snow_shot/presentation/screenshotocrpresentation.h"
#include "snow_shot/presentation/screenshotocrquickcopy.h"
#include "snow_shot/presentation/screenshotocrcopyfeedback.h"

#include <QApplication>
#include <QClipboard>
#include <QMap>
#include <QProcess>
#include <QTextEdit>
#include <QEventLoop>
#include <QTimer>
#include <cstdlib>
#include <iostream>

namespace {
void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(EXIT_FAILURE);
    }
}

ScreenshotOcrRecognitionResult textResult(const QString& text) {
    auto presentation = std::make_shared<ScreenshotOcrPresentation>();
    presentation->selection = QRect(0, 0, 64, 64);
    for (const auto& line : text.split(QChar('\n'))) {
        presentation->lines.append({line, 1.0, {}});
    }
    return {presentation};
}

class DeferredRecognition final : public ScreenshotOcrRecognitionPort {
  public:
    RequestToken recognize(ScreenshotOcrRequest request, QObject*, Completion completion) override {
        ++calls;
        require(!captureVisible, "capture closes before recognition starts");
        require(request.image.pixelColor(0, 0) == Qt::white,
                "request retains screenshot pixels after capture release");
        if (reject)
            return 0;
        const auto token = ++lastToken;
        if (synchronous)
            completion(textResult(QStringLiteral("Immediate")));
        else
            completions.insert(token, std::move(completion));
        return token;
    }
    void cancel(RequestToken token) override {
        cancelled.append(token);
    }
    bool reprioritize(RequestToken, ScreenshotOcrRequestPriority) override {
        return true;
    }
    void complete(RequestToken token, ScreenshotOcrRecognitionResult result) {
        auto completion = completions.take(token);
        require(static_cast<bool>(completion), "pending completion exists");
        completion(std::move(result));
    }
    bool captureVisible = true;
    bool synchronous = false;
    bool reject = false;
    RequestToken lastToken = 0;
    int calls = 0;
    QMap<RequestToken, Completion> completions;
    QList<RequestToken> cancelled;
};
} // namespace

void runOcrQuickCopyTests() {
    DeferredRecognition recognition;
    int copied = 0;
    ScreenshotOcrQuickCopy copy(recognition, [&] {
        ++copied;
        require(!recognition.captureVisible, "feedback is independent of the closed capture");
        require(!QApplication::clipboard()->text().isEmpty(),
                "success feedback is delivered after clipboard publication");
    });
    QImage screenshot(64, 64, QImage::Format_ARGB32);
    const QRectF rect(0, 0, 64, 64);
    auto submit = [&](const QString& formatting = QStringLiteral("keep"),
                      const QString& punctuation = QStringLiteral("keep")) {
        screenshot = QImage(64, 64, QImage::Format_ARGB32);
        screenshot.fill(Qt::white);
        recognition.captureVisible = true;
        const int calls = recognition.calls;
        const bool accepted = copy.submit(screenshot, rect, formatting, punctuation, [&] {
            recognition.captureVisible = false;
            screenshot = {};
        });
        require(recognition.calls == calls,
                "recognition waits for the capture call stack to unwind");
        QCoreApplication::processEvents();
        return accepted;
    };
    auto* clipboard = QApplication::clipboard();
    clipboard->setText(QStringLiteral("Existing clipboard"));
    require(submit(), "valid quick copy is accepted");
    require(!recognition.captureVisible && screenshot.isNull(), "capture resources are released");
    require(clipboard->text() == QStringLiteral("Existing clipboard"),
            "clipboard is unchanged while recognition is pending");
    require(copied == 0, "pending recognition must not show success");
    recognition.complete(recognition.lastToken,
                         textResult(QStringLiteral("中文第一行\nSecond line!")));
    require(clipboard->text() == QStringLiteral("中文第一行\nSecond line!"),
            "background completion copies every recognized line");
    require(copied == 1, "successful clipboard publication reports success once");
    QTextEdit pasteTarget;
    pasteTarget.paste();
    require(pasteTarget.toPlainText() == QStringLiteral("中文第一行\nSecond line!"),
            "ordinary paste consumes background OCR as plain text");
#ifdef Q_OS_MACOS
    if (QGuiApplication::platformName() == QStringLiteral("cocoa")) {
        QProcess reader;
        QEventLoop loop;
        QTimer timeout;
        timeout.setSingleShot(true);
        QObject::connect(&reader, &QProcess::finished, &loop, &QEventLoop::quit);
        QObject::connect(&timeout, &QTimer::timeout, &loop, &QEventLoop::quit);
        reader.start(QStringLiteral("/usr/bin/pbpaste"));
        timeout.start(3000);
        // Cocoa pasteboard providers need the GUI event loop while another process reads.
        loop.exec();
        require(reader.state() == QProcess::NotRunning && reader.exitCode() == 0 &&
                    QString::fromUtf8(reader.readAllStandardOutput()) ==
                        QStringLiteral("中文第一行\nSecond line!"),
                "another native process can paste the background OCR text");
    }
#endif

    require(submit(QStringLiteral("remove"), QStringLiteral("full")), "formatted copy accepted");
    recognition.complete(recognition.lastToken, textResult(QStringLiteral("A!\nB?")));
    require(clipboard->text() == QStringLiteral("A！B？"), "existing text defaults are respected");
    require(copied == 2, "formatted clipboard publication also reports success");
    require(submit(), "empty-result request accepted");
    recognition.complete(recognition.lastToken, textResult(QStringLiteral(" \n ")));
    require(clipboard->text() == QStringLiteral("A！B？"), "empty OCR preserves clipboard");
    require(submit(), "failed-result request accepted");
    recognition.complete(recognition.lastToken, {{}, QStringLiteral("Worker failed")});
    require(clipboard->text() == QStringLiteral("A！B？"), "failed OCR preserves clipboard");
    require(copied == 2, "empty and failed recognition must not show success");

    require(submit(), "first overlapping request accepted");
    const auto older = recognition.lastToken;
    require(submit(), "newer overlapping request accepted");
    require(recognition.cancelled.contains(older), "new request cancels older pending work");
    recognition.complete(recognition.lastToken, textResult(QStringLiteral("Newest")));
    recognition.complete(older, textResult(QStringLiteral("Stale")));
    require(clipboard->text() == QStringLiteral("Newest"),
            "stale result cannot overwrite clipboard");
    require(copied == 3, "stale completions must not show success");

    recognition.synchronous = true;
    require(submit(), "synchronous completion is accepted");
    const auto completedToken = recognition.lastToken;
    require(clipboard->text() == QStringLiteral("Immediate"), "synchronous result is copied");
    require(copied == 4, "synchronous completion reports success exactly once");
    recognition.synchronous = false;
    require(submit(), "next request accepted after synchronous completion");
    require(!recognition.cancelled.contains(completedToken), "completed work is not cancelled");
    recognition.complete(recognition.lastToken, textResult(QStringLiteral("After immediate")));

    recognition.reject = true;
    require(submit(), "valid request is accepted before deferred worker submission");
    require(clipboard->text() == QStringLiteral("After immediate"),
            "rejection preserves clipboard");
    require(copied == 5, "worker rejection must not show success");
    recognition.reject = false;
    bool closed = false;
    require(!copy.submit({}, rect, {}, {}, [&] { closed = true; }), "invalid image rejected");
    require(!closed, "invalid screenshot keeps capture visible");

    QImage image(64, 64, QImage::Format_ARGB32);
    image.fill(Qt::white);
    {
        ScreenshotOcrQuickCopy temporary(recognition);
        require(temporary.submit(image, rect, {}, {}, [&] { recognition.captureVisible = false; }),
                "temporary controller accepts request");
        QCoreApplication::processEvents();
    }
    const auto destroyedToken = recognition.lastToken;
    require(recognition.cancelled.contains(destroyedToken), "destruction cancels pending request");
    recognition.complete(destroyedToken, textResult(QStringLiteral("Destroyed")));
    require(clipboard->text() == QStringLiteral("After immediate"),
            "completion after destruction cannot access clipboard");

    const int callsBeforeQueued = recognition.calls;
    {
        ScreenshotOcrQuickCopy temporary(recognition);
        require(temporary.submit(image, rect, {}, {}, [&] { recognition.captureVisible = false; }),
                "queued request accepted before controller destruction");
    }
    QCoreApplication::processEvents();
    require(recognition.calls == callsBeforeQueued,
            "destroying controller before event delivery never starts OCR");

    {
        auto temporary = std::make_unique<ScreenshotOcrQuickCopy>(recognition);
        require(temporary->submit(image, rect, {}, {}, [&] { temporary.reset(); }),
                "capture teardown may destroy the quick-copy controller");
    }
    QCoreApplication::processEvents();
    require(recognition.calls == callsBeforeQueued,
            "capture teardown cannot dereference its destroyed controller");

    require(copy.submit(image, rect, {}, {}, [&] { recognition.captureVisible = false; }),
            "first queued request accepted");
    require(copy.submit(image, rect, {}, {}, [&] { recognition.captureVisible = false; }),
            "replacement queued request accepted");
    QCoreApplication::processEvents();
    require(recognition.calls == callsBeforeQueued + 1,
            "only newest queued request reaches the recognition worker");
    recognition.complete(recognition.lastToken, textResult(QStringLiteral("Queued newest")));
    require(clipboard->text() == QStringLiteral("Queued newest"),
            "replacement queued request publishes its own result");

    auto queuedPort = std::make_unique<DeferredRecognition>();
    ScreenshotOcrQuickCopy queuedMissingPort(*queuedPort);
    require(queuedMissingPort.submit(image, rect, {}, {}, [] {}),
            "live service accepts a queued request");
    queuedPort.reset();
    QCoreApplication::processEvents();
    require(clipboard->text() == QStringLiteral("Queued newest"),
            "service destroyed before queued dispatch leaves the clipboard unchanged");

    auto port = std::make_unique<DeferredRecognition>();
    ScreenshotOcrQuickCopy missingPort(*port);
    port.reset();
    require(!missingPort.submit(image, rect, {}, {}, [&] { closed = true; }),
            "destroyed recognition service rejects safely");

    const int beforeReplacement = copied;
    const auto observer = QObject::connect(clipboard, &QClipboard::dataChanged, clipboard, [&] {
        if (clipboard->text() == QStringLiteral("Replaced publication"))
            clipboard->setText(QStringLiteral("Replacement clipboard"));
    });
    require(submit(), "clipboard-observed request accepted");
    recognition.complete(recognition.lastToken, textResult(QStringLiteral("Replaced publication")));
    QObject::disconnect(observer);
    require(clipboard->text() == QStringLiteral("Replacement clipboard") &&
                copied == beforeReplacement,
            "clipboard replacement during publication must not report OCR copy success");

    ScreenshotOcrCopyFeedback feedback;
    QWidget destination;
    destination.show();
    destination.activateWindow();
    QCoreApplication::processEvents();
    const auto* active = QApplication::activeWindow();
    feedback.showCopied(QStringLiteral("Text copied"));
    QCoreApplication::processEvents();
    require(feedback.isVisible(), "copy success feedback is visible without a capture window");
    require(feedback.windowFlags().testFlag(Qt::WindowDoesNotAcceptFocus) &&
                feedback.windowFlags().testFlag(Qt::WindowTransparentForInput) &&
                QApplication::activeWindow() == active,
            "copy feedback cannot take the paste destination's focus or pointer input");
    QEventLoop expiry;
    QTimer::singleShot(900, &expiry, [&] { feedback.showCopied(QStringLiteral("Text copied")); });
    QTimer::singleShot(1700, &expiry, [&] {
        require(feedback.isVisible(), "new copy restarts feedback lifetime");
    });
    QTimer::singleShot(2600, &expiry, &QEventLoop::quit);
    expiry.exec();
    require(!feedback.isVisible(), "copy feedback disappears automatically");
}
