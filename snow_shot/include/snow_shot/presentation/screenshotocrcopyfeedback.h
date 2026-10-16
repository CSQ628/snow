#ifndef SNOW_SHOT_PRESENTATION_SCREENSHOTOCRCOPYFEEDBACK_H
#define SNOW_SHOT_PRESENTATION_SCREENSHOTOCRCOPYFEEDBACK_H

#include "widgets/message.h"
#include <QApplication>
#include <QCursor>
#include <QScreen>
#include <QTimer>
#include <QWidget>

// Independent of capture windows: publishing clipboard feedback must never reopen
// an overlay or activate Snow Shot over the user's paste destination.
class ScreenshotOcrCopyFeedback final : public QWidget {
  public:
    ScreenshotOcrCopyFeedback()
        : QWidget(nullptr, Qt::ToolTip | Qt::FramelessWindowHint | Qt::WindowDoesNotAcceptFocus |
                               Qt::WindowTransparentForInput),
          m_messages(this) {
        setObjectName(QStringLiteral("screenshot-ocr-copy-feedback"));
        setAttribute(Qt::WA_ShowWithoutActivating);
        setAttribute(Qt::WA_TransparentForMouseEvents);
        setAttribute(Qt::WA_TranslucentBackground);
        setAttribute(Qt::WA_QuitOnClose, false);
        setFocusPolicy(Qt::NoFocus);
        m_messages.setMaximumCount(1);
        m_messages.setPauseOnHover(false);
        m_expiry.setSingleShot(true);
        m_expiry.setInterval(1500);
        connect(&m_expiry, &QTimer::timeout, this, [this] {
            hide();
            m_messages.destroyAll();
        });
    }

    void showCopied(const QString& text) {
        QScreen* screen = QGuiApplication::screenAt(QCursor::pos());
        if (screen == nullptr)
            screen = QGuiApplication::primaryScreen();
        if (screen == nullptr || text.isEmpty())
            return;
        const QRect available = screen->availableGeometry();
        const int width = qMin(480, available.width());
        setGeometry(available.center().x() - width / 2, available.top() + 24, width, 96);
        setAccessibleName(text);
        show();
        adqt::widgets::AdMessage::Request request;
        request.key = QStringLiteral("screenshot-ocr-copied");
        request.content = text;
        // One timer owns both the visible lifetime and repeated-copy replacement.
        request.durationMs = 0;
        m_messages.success(std::move(request));
        m_expiry.start();
    }

  private:
    adqt::widgets::AdMessage m_messages;
    QTimer m_expiry;
};

#endif // SNOW_SHOT_PRESENTATION_SCREENSHOTOCRCOPYFEEDBACK_H
