#include "../../presentation/services/nativeprintbackend.h"

#include "snow_shot/presentation/screenshotprintservice.h"

#include <QCoreApplication>
#include <QDialog>
#include <QPainter>
#include <QPrintDialog>
#include <QPrinter>
#include <QWidget>

ScreenshotPrintService::Backend screenshotNativePrintBackend(bool) {
    return [](QWidget* owner, QImage image, ScreenshotPrintService::Completion completion) {
        if (!completion)
            return;
        if (owner == nullptr || image.isNull()) {
            completion({ScreenshotPrintService::Status::Failed, {}});
            return;
        }
        QPrinter printer(QPrinter::HighResolution);
        QPrintDialog dialog(&printer, owner);
        dialog.setWindowTitle(
            QCoreApplication::translate("ScreenshotPrintService", "Print screenshot"));
        if (dialog.exec() != QDialog::Accepted) {
            completion({ScreenshotPrintService::Status::Cancelled, {}});
            return;
        }
        QPainter painter(&printer);
        if (!painter.isActive()) {
            completion({ScreenshotPrintService::Status::Failed,
                        QCoreApplication::translate("ScreenshotPrintService",
                                                    "The printer could not be opened")});
            return;
        }
        const QRectF page = printer.pageRect(QPrinter::DevicePixel);
        painter.drawImage(ScreenshotPrintService::fittedRect(image.size(), page), image);
        completion({ScreenshotPrintService::Status::Submitted, {}});
    };
}
