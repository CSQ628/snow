#ifndef SNOW_SHOT_PRESENTATION_SCREENSHOTOCRQUICKCOPY_H
#define SNOW_SHOT_PRESENTATION_SCREENSHOTOCRQUICKCOPY_H

#include "snow_shot/presentation/screenshotocrrecognitionservice.h"
#include "snow_shot/presentation/screenshotocrtexttransform.h"
#include <QApplication>
#include <QClipboard>
#include <QPointer>
#include <QTimer>
#include <memory>
#include <utility>

// Owns background requests independently of the visible capture session.
class ScreenshotOcrQuickCopy final : public QObject {
  public:
    explicit ScreenshotOcrQuickCopy(ScreenshotOcrRecognitionPort& recognition,
                                    std::function<void()> copied = {})
        : m_recognition(&recognition), m_copied(std::move(copied)) {}

    ~ScreenshotOcrQuickCopy() override {
        cancelPending();
    }

    bool submit(QImage image, const QRectF& canvasRect, const QString& formatting,
                const QString& punctuation, const std::function<void()>& finishCapture) {
        if (m_recognition == nullptr || image.isNull() || !canvasRect.isValid() || !finishCapture ||
            !screenshotOcrImageWithinPixelLimit(image.size()))
            return false;

        cancelPending();
        const auto generation = m_generation;
        ScreenshotOcrRequest request;
        request.image = std::move(image);
        request.canvasRect = canvasRect;
        const QPointer<ScreenshotOcrQuickCopy> receiver(this);
        // The independent request owns its pixels before the capture releases them.
        finishCapture();
        // Let Cocoa/Qt finish hiding the capture and handing back activation before
        // starting OCR, including when the recognition service completes synchronously.
        // qApp owns the queued callback; its image never belongs to an overlay window.
        QTimer::singleShot(
            0, qApp,
            [receiver, generation, request = std::move(request), formatting,
             punctuation]() mutable {
                if (receiver == nullptr || generation != receiver->m_generation ||
                    receiver->m_recognition == nullptr)
                    return;
                const auto completed = std::make_shared<bool>(false);
                const auto token = receiver->m_recognition->recognize(
                    std::move(request), receiver.data(),
                    [receiver, generation, formatting, punctuation,
                     completed](ScreenshotOcrRecognitionResult result) {
                        if (receiver == nullptr || generation != receiver->m_generation ||
                            *completed)
                            return;
                        *completed = true;
                        receiver->m_token = 0;
                        if (!result.error.isEmpty() || result.presentation == nullptr)
                            return;
                        const QString text = snow_shot::presentation::applyOcrTextTransforms(
                            *result.presentation, formatting, punctuation);
                        auto* clipboard = QApplication::clipboard();
                        if (text.trimmed().isEmpty() || clipboard == nullptr)
                            return;
                        clipboard->setText(text, QClipboard::Clipboard);
                        // Clipboard observers can synchronously replace this request or
                        // destroy its owner. Only report a still-current publication.
                        if (receiver != nullptr && generation == receiver->m_generation &&
                            clipboard->text(QClipboard::Clipboard) == text) {
                            const auto copied = receiver->m_copied;
                            if (copied)
                                copied();
                        }
                    });
                if (receiver != nullptr && generation == receiver->m_generation)
                    receiver->m_token = *completed ? 0 : token;
            });
        return true;
    }

  private:
    void cancelPending() {
        ++m_generation;
        if (const auto token = std::exchange(m_token, 0); token != 0 && m_recognition != nullptr)
            m_recognition->cancel(token);
    }

    QPointer<ScreenshotOcrRecognitionPort> m_recognition;
    std::function<void()> m_copied;
    ScreenshotOcrRecognitionPort::RequestToken m_token = 0;
    quint64 m_generation = 0;
};

#endif // SNOW_SHOT_PRESENTATION_SCREENSHOTOCRQUICKCOPY_H
