// SPDX-License-Identifier: GPL-3.0-only
#include "snow_shot/presentation/components/imagespliteditorwidget.h"

#include "snow_shot/presentation/screenshotwheelinput.h"
#include "snow_shot/presentation/styles/thememanager.h"
#include "snow_shot/presentation/windowshortcutmanager.h"
#include "snow_shot/shortcuts/shortcutbinding.h"
#include "theme/theme_manager.h"
#include "widgets/button.h"
#include "widgets/form.h"
#include "widgets/input_number.h"
#include "widgets/scroll_area.h"
#include "widgets/segmented.h"

#include <QApplication>
#include <QCoreApplication>
#include <QDragEnterEvent>
#include <QDropEvent>
#include <QFileInfo>
#include <QFontMetrics>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMimeData>
#include <QMouseEvent>
#include <QPainter>
#include <QRegularExpression>
#include <QSignalBlocker>
#include <QUrl>
#include <QVBoxLayout>
#include <QWheelEvent>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <optional>

namespace snow_shot::presentation::components {
namespace {
using adqt::widgets::AdButton;
using adqt::widgets::AdForm;
using adqt::widgets::AdFormItem;
using adqt::widgets::AdInputNumber;
using adqt::widgets::AdScrollArea;
using adqt::widgets::AdSegmented;
constexpr qreal kRulerHeight = 28;
constexpr qreal kRulerWidth = 38;
constexpr qreal kLineHitDistance = 6;

QString editorText(const char* source) {
    return QCoreApplication::translate("ImageSplitEditorWidget", source);
}
QString canvasText(const char* source) {
    return QCoreApplication::translate("ImageSplitPreviewCanvas", source);
}
[[maybe_unused]] constexpr std::array kEditorTranslations{
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Split direction"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Vertical"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Horizontal"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Grid"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Split type"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Equal Split"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Precise Split"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Number of Images"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Rows"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Columns"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Width per Image"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Height per Image"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Pixels"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Percentage"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", " px"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Split size unit"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Enter a positive integer."),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Enter an integer of at least 16 pixels."),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Undo"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Redo"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Fit to Canvas"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Reload"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "%1 × %2 px"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Total: %1 images"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Split lines adjusted manually"),
    QT_TRANSLATE_NOOP("ImageSplitEditorWidget", "Drop one image to replace the current image"),
};
[[maybe_unused]] constexpr std::array kCanvasTranslations{
    QT_TRANSLATE_NOOP("ImageSplitPreviewCanvas", "Image split preview"),
    QT_TRANSLATE_NOOP(
        "ImageSplitPreviewCanvas",
        "Click a ruler to add a split line; drag a line to move it. Select a line and press Delete "
        "to remove it. Drag with the middle mouse button to pan; scroll to zoom."),
    QT_TRANSLATE_NOOP("ImageSplitPreviewCanvas", "X: %1 px"),
    QT_TRANSLATE_NOOP("ImageSplitPreviewCanvas", "Y: %1 px"),
};

QStringList droppedPaths(const QMimeData* mime) {
    QStringList paths;
    if (!mime || !mime->hasUrls())
        return paths;
    for (const auto& url : mime->urls()) {
        if (!url.isLocalFile())
            return {};
        paths.append(url.toLocalFile());
    }
    return paths;
}

int roundedCoordinate(qreal coordinate) {
    return static_cast<int>(
        std::clamp(std::floor(coordinate + .5), 0.0, qreal(std::numeric_limits<int>::max())));
}
} // namespace

struct ImageSplitPreviewCanvas::Impl {
    ImageSplitPreviewCanvas* owner = nullptr;
    ImageSplitDocument* document = nullptr;
    qreal zoom = 1;
    QPointF offset;
    QPointF last;
    bool automaticFit = true;
    bool panning = false;
    bool dragging = false;
    bool editing = true;
    bool selectedVertical = true;
    int selectedIndex = -1;
    bool hoveredVertical = true;
    int hoveredIndex = -1;
    qint64 imageKey = 0;
    ImageSplitSettings previousSettings;

    QRectF viewport() const {
        return QRectF(kRulerWidth, kRulerHeight, std::max(1.0, owner->width() - kRulerWidth),
                      std::max(1.0, owner->height() - kRulerHeight));
    }
    bool axisAllowed(bool vertical) const {
        const auto direction = document->settings().direction;
        return direction == ImageSplitDirection::Grid ||
               (vertical ? direction == ImageSplitDirection::Vertical
                         : direction == ImageSplitDirection::Horizontal);
    }
    const auto& lines(const ImageSplitLayout& layout, bool vertical) const {
        return vertical ? layout.verticalLines : layout.horizontalLines;
    }
    std::pair<bool, int> hit(QPointF point) const {
        const auto layout = document->layout();
        if (!viewport().contains(point))
            return {true, -1};
        const auto imageRect = QRectF(owner->imageToViewport({}), QSizeF(layout.size) * zoom);
        if (!imageRect
                 .adjusted(-kLineHitDistance, -kLineHitDistance, kLineHitDistance, kLineHitDistance)
                 .contains(point))
            return {true, -1};
        bool axis = true;
        int result = -1;
        qreal closest = kLineHitDistance;
        const auto imagePoint = owner->viewportToImage(point);
        for (const bool vertical : {true, false}) {
            if (!axisAllowed(vertical))
                continue;
            const auto& boundaries = lines(layout, vertical);
            const qreal value = vertical ? imagePoint.x() : imagePoint.y();
            const auto candidate = std::lower_bound(boundaries.cbegin(), boundaries.cend(), value);
            for (int shift : {-1, 0}) {
                const int index = static_cast<int>(candidate - boundaries.cbegin()) + shift;
                if (index < 0 || index >= boundaries.size())
                    continue;
                const qreal distance = std::abs(boundaries[index] - value) * zoom;
                if (distance <= closest) {
                    closest = distance;
                    result = index;
                    axis = vertical;
                }
            }
        }
        return {axis, result};
    }
    void refresh() {
        const auto key = document->image().cacheKey();
        const auto settings = document->settings();
        if (key != imageKey) {
            imageKey = key;
            selectedIndex = -1;
            hoveredIndex = -1;
            owner->fitToCanvas();
        } else if (!(settings == previousSettings)) {
            selectedIndex = -1;
            hoveredIndex = -1;
        }
        previousSettings = settings;
        const auto layout = document->layout();
        if (selectedIndex >= lines(layout, selectedVertical).size())
            selectedIndex = -1;
        if (hoveredIndex >= lines(layout, hoveredVertical).size())
            hoveredIndex = -1;
        owner->update();
    }
};

ImageSplitPreviewCanvas::ImageSplitPreviewCanvas(ImageSplitDocument* document, QWidget* parent)
    : QWidget(parent), m_impl(std::make_unique<Impl>()) {
    auto& d = *m_impl;
    d.owner = this;
    d.document = document;
    setObjectName(QStringLiteral("imageSplitCanvas"));
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
    setMinimumSize(180, 160);
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    connect(document, &ImageSplitDocument::changed, this, [this] { m_impl->refresh(); });
    connect(&adqt::theme::ThemeManager::instance(), &adqt::theme::ThemeManager::themeChanged, this,
            [this] { update(); });
    retranslateUi();
    d.refresh();
}
ImageSplitPreviewCanvas::~ImageSplitPreviewCanvas() = default;
qreal ImageSplitPreviewCanvas::zoom() const {
    return m_impl->zoom;
}
QPointF ImageSplitPreviewCanvas::offset() const {
    return m_impl->offset;
}
QPointF ImageSplitPreviewCanvas::imageToViewport(QPointF point) const {
    return m_impl->offset + point * m_impl->zoom;
}
QPointF ImageSplitPreviewCanvas::viewportToImage(QPointF point) const {
    return (point - m_impl->offset) / m_impl->zoom;
}
void ImageSplitPreviewCanvas::fitToCanvas() {
    auto& d = *m_impl;
    if (d.document->image().isNull())
        return;
    const auto area = d.viewport().adjusted(24, 24, -24, -24);
    const auto size = d.document->image().size();
    d.automaticFit = true;
    d.zoom = std::min({1.0, std::max(1.0, area.width()) / size.width(),
                       std::max(1.0, area.height()) / size.height()});
    d.offset = d.viewport().center() - QPointF(size.width(), size.height()) * d.zoom / 2;
    update();
}
void ImageSplitPreviewCanvas::setEditingEnabled(bool enabled) {
    auto& d = *m_impl;
    if (d.editing == enabled)
        return;
    if (!enabled && d.dragging)
        d.document->cancelLineDrag();
    d.dragging = false;
    d.editing = enabled;
    d.hoveredIndex = -1;
    unsetCursor();
    update();
}
void ImageSplitPreviewCanvas::retranslateUi() {
    setAccessibleName(canvasText("Image split preview"));
    setAccessibleDescription(canvasText(
        "Click a ruler to add a split line; drag a line to move it. Select a line and press Delete "
        "to remove it. Drag with the middle mouse button to pan; scroll to zoom."));
    setToolTip(accessibleDescription());
    update();
}

void ImageSplitPreviewCanvas::paintEvent(QPaintEvent*) {
    const auto& d = *m_impl;
    const auto theme = adqt::theme::ThemeManager::instance().resolveTheme(this);
    QPainter painter(this);
    painter.fillRect(rect(), theme.colorFillSecondary);
    const auto viewport = d.viewport();
    const auto size = d.document->image().size();
    const auto layout = d.document->layout();
    painter.save();
    painter.setClipRect(viewport);
    const QRectF imageRect(d.offset, QSizeF(size) * d.zoom);
    if (!size.isEmpty()) {
        // The checker is a small logical-pixel tile, independent of image size and canvas zoom.
        QPixmap tile(20, 20);
        tile.fill(theme.colorBgContainer);
        QPainter checker(&tile);
        checker.fillRect(0, 0, 10, 10, theme.colorFillSecondary);
        checker.fillRect(10, 10, 10, 10, theme.colorFillSecondary);
        checker.end();
        painter.fillRect(imageRect, QBrush(tile));
        const auto& preview = d.document->preview();
        const qreal previewScale = preview.isNull()
                                       ? 0
                                       : std::min(qreal(preview.width()) / size.width(),
                                                  qreal(preview.height()) / size.height());
        const qreal displayScale = d.zoom * devicePixelRatioF();
        painter.setRenderHint(QPainter::SmoothPixmapTransform, displayScale < 1);
        if (!preview.isNull() && displayScale < previewScale) {
            painter.drawImage(imageRect, preview);
        } else {
            // Draw only the visible source pixels when the preview no longer has enough detail.
            // This avoids allocating a full-size pixmap or scaling the source on the GUI thread.
            const QRectF visiblePixels =
                QRectF(viewportToImage(viewport.topLeft()), viewportToImage(viewport.bottomRight()))
                    .intersected(QRectF(QPointF{}, QSizeF(size)));
            if (!visiblePixels.isEmpty()) {
                const QRectF visibleTarget(imageToViewport(visiblePixels.topLeft()),
                                           visiblePixels.size() * d.zoom);
                painter.drawImage(visibleTarget, d.document->image(), visiblePixels);
            }
        }
        painter.setPen(QPen(theme.colorBorder, 1));
        painter.drawRect(imageRect);
        const auto drawLines = [&](bool vertical) {
            const auto& lines = d.lines(layout, vertical);
            const qreal visibleFirst = vertical ? viewportToImage(viewport.topLeft()).x()
                                                : viewportToImage(viewport.topLeft()).y();
            const qreal visibleLast = vertical ? viewportToImage(viewport.bottomRight()).x()
                                               : viewportToImage(viewport.bottomRight()).y();
            const auto drawLine = [&](int coordinate, bool highlighted) {
                const qreal position = vertical ? imageToViewport({qreal(coordinate), 0}).x()
                                                : imageToViewport({0, qreal(coordinate)}).y();
                painter.setPen(QPen(highlighted ? theme.colorPrimaryHover : theme.colorPrimary,
                                    highlighted ? 2 : 1, Qt::DashLine));
                if (vertical)
                    painter.drawLine(QPointF(position, imageRect.top()),
                                     QPointF(position, imageRect.bottom()));
                else
                    painter.drawLine(QPointF(imageRect.left(), position),
                                     QPointF(imageRect.right(), position));
            };
            auto line = std::lower_bound(lines.cbegin(), lines.cend(), visibleFirst);
            while (line != lines.cend() && *line <= visibleLast) {
                drawLine(*line, false);
                // Jump over subpixel guides instead of traversing a potentially dense axis.
                const qreal nextCoordinate = *line + std::max(1.0, 2 / d.zoom);
                line = std::lower_bound(line + 1, lines.cend(), nextCoordinate);
            }
            for (const int index : {d.selectedVertical == vertical ? d.selectedIndex : -1,
                                    d.hoveredVertical == vertical ? d.hoveredIndex : -1}) {
                if (index >= 0 && index < lines.size() && lines[index] >= visibleFirst &&
                    lines[index] <= visibleLast)
                    drawLine(lines[index], true);
            }
        };
        drawLines(true);
        drawLines(false);
        QFont numberFont = font();
        numberFont.setPixelSize(12);
        painter.setFont(numberFont);
        const QFontMetrics metrics(numberFont);
        const auto readableCells = [&](bool horizontal, qreal minimumSpan) {
            QVector<int> result;
            const auto& lines = horizontal ? layout.verticalLines : layout.horizontalLines;
            const int dimension = horizontal ? layout.size.width() : layout.size.height();
            const qreal first = horizontal ? viewport.left() : viewport.top();
            const qreal last = horizontal ? viewport.right() : viewport.bottom();
            const qreal origin = horizontal ? d.offset.x() : d.offset.y();
            const qreal step = minimumSpan / 2;
            int previous = -1;
            const auto sample = [&](qreal position) {
                const qreal coordinate = (position - origin) / d.zoom;
                if (coordinate < 0 || coordinate > dimension)
                    return;
                const int index = static_cast<int>(
                    std::upper_bound(lines.cbegin(), lines.cend(), coordinate) - lines.cbegin());
                if (index == previous)
                    return;
                previous = index;
                const int start = index == 0 ? 0 : lines[index - 1];
                const int end = index == lines.size() ? dimension : lines[index];
                if ((end - start) * d.zoom >= minimumSpan)
                    result.append(index);
            };
            // A readable cell contains a sample, even after thousands of subpixel cells. Work is
            // bounded by viewport pixels, with no allocation or traversal of the region product.
            for (qreal position = first; position < last; position += step)
                sample(position);
            sample(last);
            return result;
        };
        const auto readableColumns = readableCells(true, metrics.horizontalAdvance(u'1') + 16);
        const auto readableRows = readableCells(false, 26);
        const qint64 columns = layout.verticalLines.size() + 1;
        for (const int row : readableRows) {
            for (const int column : readableColumns) {
                const auto index = row * columns + column;
                const auto region = layout.regionAt(index);
                const QRectF target(imageToViewport(region.topLeft()),
                                    QSizeF(region.size()) * d.zoom);
                const auto label = QString::number(index + 1);
                const QSizeF badge(metrics.horizontalAdvance(label) + 10, 20);
                if (target.width() < badge.width() + 6)
                    continue;
                const QRectF badgeRect(target.topLeft() + QPointF(4, 4), badge);
                if (!badgeRect.intersects(viewport))
                    continue;
                painter.setRenderHint(QPainter::Antialiasing);
                painter.setPen(Qt::NoPen);
                painter.setBrush(theme.colorBgElevated);
                painter.drawRoundedRect(badgeRect, 3, 3);
                painter.setPen(theme.colorTextSecondary);
                painter.drawText(badgeRect, Qt::AlignCenter, label);
            }
        }
        const int labelIndex = d.hoveredIndex >= 0 ? d.hoveredIndex : d.selectedIndex;
        const bool labelVertical = d.hoveredIndex >= 0 ? d.hoveredVertical : d.selectedVertical;
        const auto& labelLines = d.lines(layout, labelVertical);
        if (labelIndex >= 0 && labelIndex < labelLines.size()) {
            const int coordinate = labelLines[labelIndex];
            const auto label =
                (labelVertical ? canvasText("X: %1 px") : canvasText("Y: %1 px")).arg(coordinate);
            const QSizeF badge(metrics.horizontalAdvance(label) + 12, 22);
            auto anchor = labelVertical ? imageToViewport({qreal(coordinate), 0})
                                        : imageToViewport({0, qreal(coordinate)});
            anchor += QPointF(6, 6);
            anchor.setX(
                std::clamp(anchor.x(), viewport.left() + 2,
                           std::max(viewport.left() + 2, viewport.right() - badge.width() - 2)));
            anchor.setY(
                std::clamp(anchor.y(), viewport.top() + 2,
                           std::max(viewport.top() + 2, viewport.bottom() - badge.height() - 2)));
            painter.setPen(Qt::NoPen);
            painter.setBrush(theme.colorPrimary);
            const QRectF badgeRect(anchor, badge);
            painter.drawRoundedRect(badgeRect, 3, 3);
            painter.setPen(theme.colorTextLightSolid);
            painter.drawText(badgeRect, Qt::AlignCenter, label);
        }
    }
    painter.restore();
    painter.fillRect(QRectF(kRulerWidth, 0, width() - kRulerWidth, kRulerHeight),
                     theme.colorBgContainer);
    painter.fillRect(QRectF(0, kRulerHeight, kRulerWidth, height() - kRulerHeight),
                     theme.colorBgContainer);
    painter.fillRect(QRectF(0, 0, kRulerWidth, kRulerHeight), theme.colorBgContainer);
    painter.setPen(theme.colorBorderSecondary);
    painter.drawLine(QPointF(kRulerWidth, kRulerHeight), QPointF(width(), kRulerHeight));
    painter.drawLine(QPointF(kRulerWidth, kRulerHeight), QPointF(kRulerWidth, height()));
    if (size.isEmpty())
        return;
    QFont rulerFont = font();
    rulerFont.setPixelSize(10);
    painter.setFont(rulerFont);
    for (const bool horizontal : {true, false}) {
        const qreal desired = 80 / d.zoom;
        const qreal magnitude = std::pow(10.0, std::floor(std::log10(std::max(1.0, desired))));
        qreal major = magnitude;
        for (qreal factor : {1.0, 2.0, 5.0, 10.0}) {
            if (magnitude * factor >= desired) {
                major = magnitude * factor;
                break;
            }
        }
        major = std::max(1.0, major);
        const qreal minor = std::max(1.0, major / 5);
        const qreal first = horizontal ? viewportToImage(viewport.topLeft()).x()
                                       : viewportToImage(viewport.topLeft()).y();
        const qreal last = horizontal ? viewportToImage(viewport.bottomRight()).x()
                                      : viewportToImage(viewport.bottomRight()).y();
        const qreal dimension = horizontal ? size.width() : size.height();
        const qreal begin = std::max(0.0, std::ceil(first / minor) * minor);
        for (qreal coordinate = begin; coordinate <= std::min(last, dimension);
             coordinate += minor) {
            const auto point =
                imageToViewport(horizontal ? QPointF(coordinate, 0) : QPointF(0, coordinate));
            const bool majorTick =
                std::abs(coordinate / major - std::round(coordinate / major)) < .001;
            painter.setPen(theme.colorTextTertiary);
            if (horizontal) {
                painter.drawLine(QPointF(point.x(), kRulerHeight),
                                 QPointF(point.x(), kRulerHeight - (majorTick ? 8 : 4)));
                if (majorTick)
                    painter.drawText(QRectF(point.x() - 35, 1, 70, 16), Qt::AlignCenter,
                                     QString::number(roundedCoordinate(coordinate)));
            } else {
                painter.drawLine(QPointF(kRulerWidth, point.y()),
                                 QPointF(kRulerWidth - (majorTick ? 8 : 4), point.y()));
                if (majorTick) {
                    painter.save();
                    painter.translate(16, point.y());
                    painter.rotate(-90);
                    painter.drawText(QRectF(-35, -8, 70, 16), Qt::AlignCenter,
                                     QString::number(roundedCoordinate(coordinate)));
                    painter.restore();
                }
            }
        }
    }
}

void ImageSplitPreviewCanvas::resizeEvent(QResizeEvent*) {
    if (m_impl->automaticFit)
        fitToCanvas();
    update();
}
void ImageSplitPreviewCanvas::mousePressEvent(QMouseEvent* event) {
    auto& d = *m_impl;
    setFocus(Qt::MouseFocusReason);
    if (event->button() == Qt::MiddleButton) {
        d.panning = true;
        d.automaticFit = false;
        d.last = event->position();
        setCursor(Qt::ClosedHandCursor);
        event->accept();
        return;
    }
    if (event->button() != Qt::LeftButton || !d.editing || d.document->image().isNull())
        return;
    const auto point = event->position();
    const auto pixel = viewportToImage(point);
    const auto create = [&d](bool vertical, int coordinate) {
        static_cast<void>(d.document->createLine(vertical, coordinate));
        const auto layout = d.document->layout();
        const auto& lines = d.lines(layout, vertical);
        const auto line = std::lower_bound(lines.cbegin(), lines.cend(), coordinate);
        if (line != lines.cend() && *line == coordinate) {
            d.selectedVertical = vertical;
            d.selectedIndex = static_cast<int>(line - lines.cbegin());
            d.hoveredIndex = -1;
        }
    };
    if (point.y() < kRulerHeight && point.x() >= kRulerWidth && d.axisAllowed(true)) {
        create(true, roundedCoordinate(pixel.x()));
    } else if (point.x() < kRulerWidth && point.y() >= kRulerHeight && d.axisAllowed(false)) {
        create(false, roundedCoordinate(pixel.y()));
    } else {
        const auto [vertical, index] = d.hit(point);
        d.selectedVertical = vertical;
        d.selectedIndex = index;
        if (index >= 0 && d.document->beginLineDrag(vertical, index)) {
            d.dragging = true;
            setCursor(vertical ? Qt::SizeHorCursor : Qt::SizeVerCursor);
        }
    }
    update();
    event->accept();
}
void ImageSplitPreviewCanvas::mouseMoveEvent(QMouseEvent* event) {
    auto& d = *m_impl;
    if (d.panning) {
        d.offset += event->position() - d.last;
        d.last = event->position();
        update();
    } else if (d.dragging) {
        const auto point = viewportToImage(event->position());
        d.document->moveDraggedLine(roundedCoordinate(d.selectedVertical ? point.x() : point.y()));
    } else if (d.editing) {
        const auto [vertical, index] = d.hit(event->position());
        d.hoveredVertical = vertical;
        d.hoveredIndex = index;
        if (index >= 0)
            setCursor(vertical ? Qt::SizeHorCursor : Qt::SizeVerCursor);
        else if ((event->position().y() < kRulerHeight && d.axisAllowed(true)) ||
                 (event->position().x() < kRulerWidth && d.axisAllowed(false)))
            setCursor(Qt::CrossCursor);
        else
            unsetCursor();
        update();
    }
}
void ImageSplitPreviewCanvas::mouseReleaseEvent(QMouseEvent* event) {
    auto& d = *m_impl;
    if (event->button() == Qt::MiddleButton)
        d.panning = false;
    if (event->button() == Qt::LeftButton && d.dragging) {
        d.document->endLineDrag();
        d.dragging = false;
    }
    unsetCursor();
    update();
}
void ImageSplitPreviewCanvas::wheelEvent(QWheelEvent* event) {
    auto& d = *m_impl;
    if (d.document->image().isNull())
        return;
    const double steps = usesPreciseWheelDelta(*event) ? event->pixelDelta().y() / 100.0
                                                       : event->angleDelta().y() / 120.0;
    if (steps != 0) {
        const qreal next = std::clamp(d.zoom * std::pow(1.15, steps), 0.000000001, 16.0);
        d.offset = event->position() - (event->position() - d.offset) * (next / d.zoom);
        d.zoom = next;
        d.automaticFit = false;
        update();
    }
    event->accept();
}
void ImageSplitPreviewCanvas::keyPressEvent(QKeyEvent* event) {
    auto& d = *m_impl;
    if (event->key() == Qt::Key_Escape && d.dragging) {
        d.document->cancelLineDrag();
        d.dragging = false;
        unsetCursor();
    } else if ((event->key() == Qt::Key_Delete || event->key() == Qt::Key_Backspace) && d.editing) {
        if (d.selectedIndex >= 0)
            d.document->removeLine(d.selectedVertical, d.selectedIndex);
        d.selectedIndex = -1;
        d.hoveredIndex = -1;
    } else if (event->key() == Qt::Key_F || event->key() == Qt::Key_Home) {
        fitToCanvas();
    } else if (d.editing && shortcuts::matchesStandardShortcut(*event, QKeySequence::Undo)) {
        d.document->undo();
    } else if (d.editing && shortcuts::matchesStandardShortcut(*event, QKeySequence::Redo)) {
        d.document->redo();
    } else {
        QWidget::keyPressEvent(event);
        return;
    }
    event->accept();
    update();
}
void ImageSplitPreviewCanvas::leaveEvent(QEvent*) {
    m_impl->hoveredIndex = -1;
    if (!m_impl->dragging && !m_impl->panning)
        unsetCursor();
    update();
}
void ImageSplitPreviewCanvas::changeEvent(QEvent* event) {
    if (event->type() == QEvent::LanguageChange)
        retranslateUi();
    QWidget::changeEvent(event);
}

struct ImageSplitEditorWidget::Impl {
    ImageSplitEditorWidget* owner = nullptr;
    ImageSplitDocument* document = nullptr;
    ImageSplitPreviewCanvas* canvas = nullptr;
    AdForm* form = nullptr;
    AdSegmented* direction = nullptr;
    AdSegmented* type = nullptr;
    AdSegmented* unit = nullptr;
    AdFormItem* directionField = nullptr;
    AdFormItem* typeField = nullptr;
    AdFormItem* countField = nullptr;
    AdFormItem* rowsField = nullptr;
    AdFormItem* columnsField = nullptr;
    AdFormItem* sizeField = nullptr;
    AdInputNumber* count = nullptr;
    AdInputNumber* rows = nullptr;
    AdInputNumber* columns = nullptr;
    AdInputNumber* size = nullptr;
    QLabel* sizeLabel = nullptr;
    QLabel* manual = nullptr;
    QLabel* dropHint = nullptr;
    QWidget* description = nullptr;
    QLabel* dimensions = nullptr;
    QLabel* total = nullptr;
    std::array<AdButton*, 4> actions{};
    QMap<QString, QString> drafts;
    bool syncing = false;
    bool loading = false;
    bool exporting = false;
    bool valid = false;
    qint64 imageKey = 0;
    ImageSplitSettings lastSettings;
    ImageSplitUnit displayedUnit = ImageSplitUnit::Percentage;
    mutable std::optional<ImageSplitSettings> validatedSettings;
    mutable QSize validatedSize;
    mutable QMap<QString, QString> cachedErrors;

    bool busy() const {
        return loading || exporting;
    }
    QString sizeKey() const {
        return unit->currentValue() == QStringLiteral("pixels") ? QStringLiteral("pixels")
                                                                : QStringLiteral("percentage");
    }
    std::pair<ImageSplitSettings, QMap<QString, QString>> draftSettings() const {
        auto settings = document->settings();
        settings.direction = direction->currentValue() == QStringLiteral("horizontal")
                                 ? ImageSplitDirection::Horizontal
                             : direction->currentValue() == QStringLiteral("grid")
                                 ? ImageSplitDirection::Grid
                                 : ImageSplitDirection::Vertical;
        settings.type = type->currentValue() == QStringLiteral("precise") ? ImageSplitType::Precise
                                                                          : ImageSplitType::Equal;
        settings.unit = unit->currentValue() == QStringLiteral("pixels")
                            ? ImageSplitUnit::Pixels
                            : ImageSplitUnit::Percentage;
        QMap<QString, QString> errors;
        const auto integer = [&](const QString& key, int& destination, int minimum) {
            const auto draft = drafts.value(key).trimmed();
            static const QRegularExpression pattern(QStringLiteral("^[0-9]+$"));
            bool ok = false;
            const qlonglong value = draft.toLongLong(&ok);
            if (!pattern.match(draft).hasMatch() || !ok || value < minimum ||
                value > std::numeric_limits<int>::max()) {
                errors.insert(key, minimum == 16
                                       ? editorText("Enter an integer of at least 16 pixels.")
                                       : editorText("Enter a positive integer."));
            } else {
                destination = static_cast<int>(value);
            }
        };
        if (settings.direction == ImageSplitDirection::Grid) {
            integer(QStringLiteral("rows"), settings.rows, 1);
            integer(QStringLiteral("columns"), settings.columns, 1);
        } else if (settings.type == ImageSplitType::Equal) {
            integer(QStringLiteral("count"), settings.count, 1);
        } else if (settings.unit == ImageSplitUnit::Pixels) {
            integer(QStringLiteral("pixels"), settings.pixels, 16);
        } else {
            // The model validates and calculates the complete decimal exactly. Only conversion
            // from pixels formats two decimal places; user-entered precision remains intact.
            settings.percentage = drafts.value(QStringLiteral("percentage")).trimmed();
        }
        if (errors.isEmpty()) {
            if (!validatedSettings || !(*validatedSettings == settings) ||
                validatedSize != document->image().size()) {
                validatedSettings = settings;
                validatedSize = document->image().size();
                cachedErrors = calculateImageSplitLayout(validatedSize, settings).errors;
            }
            errors = cachedErrors;
        }
        return {settings, errors};
    }
    void displayError(AdFormItem* field, AdInputNumber* input, const QString& key,
                      const QMap<QString, QString>& errors) {
        const auto error = errors.value(key);
        field->setErrorMessages(error.isEmpty() ? QStringList{} : QStringList{error});
        field->setValidateStatus(error.isEmpty() ? AdFormItem::ValidateStatus::None
                                                 : AdFormItem::ValidateStatus::Error);
        input->setStatus(error.isEmpty() ? AdInputNumber::Status::None
                                         : AdInputNumber::Status::Error);
    }
    void validate() {
        const auto [settings, errors] = draftSettings();
        Q_UNUSED(settings);
        displayError(countField, count, QStringLiteral("count"), errors);
        displayError(rowsField, rows, QStringLiteral("rows"), errors);
        displayError(columnsField, columns, QStringLiteral("columns"), errors);
        displayError(sizeField, size, sizeKey(), errors);
        const bool next = errors.isEmpty() && document->layout().isValid();
        if (valid != next) {
            valid = next;
            emit owner->validationChanged();
        }
        updateActions();
    }
    void commit() {
        if (syncing || busy())
            return;
        const auto [settings, errors] = draftSettings();
        if (errors.isEmpty() && !(settings == document->settings()))
            document->setSettings(settings);
        validate();
    }
    void updateActions() {
        const bool available = !busy() && !document->image().isNull();
        actions[0]->setEnabled(available && document->canUndo());
        actions[1]->setEnabled(available && document->canRedo());
        actions[2]->setEnabled(!document->image().isNull());
        actions[3]->setEnabled(!busy());
        form->setEnabled(!busy());
        const auto displayedDirection = direction->currentValue() == QStringLiteral("horizontal")
                                            ? ImageSplitDirection::Horizontal
                                        : direction->currentValue() == QStringLiteral("grid")
                                            ? ImageSplitDirection::Grid
                                            : ImageSplitDirection::Vertical;
        canvas->setEditingEnabled(available && document->layout().isValid() &&
                                  displayedDirection == document->settings().direction);
        if (busy())
            dropHint->hide();
    }
    void updateVisibility() {
        const bool grid = direction->currentValue() == QStringLiteral("grid");
        const bool equal = type->currentValue() == QStringLiteral("equal");
        typeField->setItemHidden(grid);
        countField->setItemHidden(grid || !equal);
        sizeField->setItemHidden(grid || equal);
        rowsField->setItemHidden(!grid);
        columnsField->setItemHidden(!grid);
        sizeLabel->setText(direction->currentValue() == QStringLiteral("horizontal")
                               ? editorText("Height per Image")
                               : editorText("Width per Image"));
        size->setAccessibleName(sizeLabel->text());
    }
    void sync() {
        syncing = true;
        const auto settings = document->settings();
        direction->setCurrentValue(
            settings.direction == ImageSplitDirection::Vertical     ? QStringLiteral("vertical")
            : settings.direction == ImageSplitDirection::Horizontal ? QStringLiteral("horizontal")
                                                                    : QStringLiteral("grid"));
        type->setCurrentValue(settings.type == ImageSplitType::Equal ? QStringLiteral("equal")
                                                                     : QStringLiteral("precise"));
        unit->setCurrentValue(settings.unit == ImageSplitUnit::Pixels
                                  ? QStringLiteral("pixels")
                                  : QStringLiteral("percentage"));
        count->setValue(settings.count);
        rows->setValue(settings.rows);
        columns->setValue(settings.columns);
        size->setDecimals(-1);
        size->setValueMode(settings.unit == ImageSplitUnit::Pixels
                               ? AdInputNumber::ValueMode::Number
                               : AdInputNumber::ValueMode::ExactDecimal);
        size->setRange(settings.unit == ImageSplitUnit::Pixels ? 16 : 5,
                       settings.unit == ImageSplitUnit::Pixels ? std::numeric_limits<int>::max()
                                                               : 100);
        size->setSuffixText(settings.unit == ImageSplitUnit::Pixels ? editorText(" px")
                                                                    : QStringLiteral("%"));
        if (settings.unit == ImageSplitUnit::Pixels)
            size->setValue(settings.pixels);
        else
            size->setExactValue(settings.percentage);
        drafts.insert(QStringLiteral("count"), QString::number(settings.count));
        drafts.insert(QStringLiteral("rows"), QString::number(settings.rows));
        drafts.insert(QStringLiteral("columns"), QString::number(settings.columns));
        drafts.insert(QStringLiteral("pixels"), QString::number(settings.pixels));
        drafts.insert(QStringLiteral("percentage"), settings.percentage);
        updateVisibility();
        lastSettings = settings;
        displayedUnit = settings.unit;
        imageKey = document->image().cacheKey();
        syncing = false;
    }
    void refresh() {
        if (!(document->settings() == lastSettings) || document->image().cacheKey() != imageKey)
            sync();
        const auto layout = document->layout();
        dimensions->setText(
            editorText("%1 × %2 px").arg(layout.size.width()).arg(layout.size.height()));
        total->setText(editorText("Total: %1 images").arg(layout.outputCount()));
        manual->setVisible(document->state().manuallyEdited);
        validate();
        updateActions();
    }
    void changeUnit(const QVariant& value) {
        if (syncing || busy())
            return;
        const bool toPixels = value.toString() == QStringLiteral("pixels");
        if (toPixels == (displayedUnit == ImageSplitUnit::Pixels))
            return;
        const auto sourceSize = document->image().size();
        const int dimension = direction->currentValue() == QStringLiteral("horizontal")
                                  ? sourceSize.height()
                                  : sourceSize.width();
        QString converted;
        if (toPixels) {
            const auto source = drafts.value(QStringLiteral("percentage")).trimmed();
            if (const auto pixels = imageSplitPercentagePixels(dimension, source))
                converted = QString::number(*pixels);
            drafts.insert(QStringLiteral("pixels"), converted);
        } else {
            bool ok = false;
            const qint64 pixels = drafts.value(QStringLiteral("pixels")).trimmed().toLongLong(&ok);
            if (ok && pixels >= 0 && pixels <= std::numeric_limits<int>::max() && dimension > 0) {
                const qint64 hundredths = (pixels * 10000 + dimension / 2) / dimension;
                converted = QStringLiteral("%1.%2")
                                .arg(hundredths / 100)
                                .arg(hundredths % 100, 2, 10, QChar(u'0'));
            }
            drafts.insert(QStringLiteral("percentage"), converted);
        }
        displayedUnit = toPixels ? ImageSplitUnit::Pixels : ImageSplitUnit::Percentage;
        syncing = true;
        size->setValueMode(toPixels ? AdInputNumber::ValueMode::Number
                                    : AdInputNumber::ValueMode::ExactDecimal);
        size->setRange(toPixels ? 16 : 5, toPixels ? std::numeric_limits<int>::max() : 100);
        size->setSuffixText(toPixels ? editorText(" px") : QStringLiteral("%"));
        if (converted.isEmpty())
            size->clear();
        else if (toPixels)
            size->setValue(converted.toDouble());
        else
            size->setExactValue(converted);
        syncing = false;
        commit();
    }
    void theme() {
        const auto theme = adqt::theme::ThemeManager::instance().resolveTheme(owner);
        QPalette palette = owner->palette();
        palette.setColor(QPalette::WindowText, theme.colorTextSecondary);
        palette.setColor(QPalette::Disabled, QPalette::WindowText, theme.colorTextDisabled);
        for (auto* label : {manual, dimensions, total})
            label->setPalette(palette);
        dropHint->setPalette(palette);
        owner->update();
    }
};

ImageSplitEditorWidget::ImageSplitEditorWidget(ImageSplitDocument* document, QWidget* parent)
    : QWidget(parent), m_impl(std::make_unique<Impl>()) {
    auto& d = *m_impl;
    d.owner = this;
    d.document = document;
    setObjectName(QStringLiteral("imageSplitEditorWidget"));
    setAcceptDrops(true);
    const auto metric = styles::ThemeManager::instance().themeColorScheme().metricAlias;
    auto* outer = new QHBoxLayout(this);
    outer->setContentsMargins(0, 0, 0, 0);
    outer->setSpacing(metric.marginMD);
    auto* previewColumn = new QVBoxLayout;
    previewColumn->setContentsMargins(0, 0, 0, 0);
    previewColumn->setSpacing(metric.marginXS);
    d.canvas = new ImageSplitPreviewCanvas(document, this);
    previewColumn->addWidget(d.canvas, 1);
    auto* actions = new QHBoxLayout;
    actions->setContentsMargins(0, 0, 0, 0);
    actions->setSpacing(metric.marginXS);
    const std::array<const char*, 4> names{"imageSplitUndo", "imageSplitRedo", "imageSplitFit",
                                           "imageSplitReload"};
    for (std::size_t i = 0; i < d.actions.size(); ++i) {
        auto* button = new AdButton(this);
        button->setObjectName(QString::fromLatin1(names[i]));
        button->setButtonStyle(AdButton::ButtonStyle::Text);
        button->setFocusPolicy(Qt::StrongFocus);
        d.actions[i] = button;
        actions->addWidget(button);
    }
    actions->addStretch(1);
    previewColumn->addLayout(actions);
    outer->addLayout(previewColumn, 1);
    auto* scroll = new AdScrollArea(this);
    scroll->setObjectName(QStringLiteral("imageSplitSettingsScroll"));
    scroll->setFixedWidth(340);
    scroll->setWidgetResizable(true);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->viewport()->setAutoFillBackground(false);
    d.form = new AdForm;
    d.form->setObjectName(QStringLiteral("imageSplitSettingsForm"));
    d.form->setFormLayout(AdForm::FormLayout::Vertical);
    d.form->setLabelAlign(AdForm::LabelAlign::Left);
    d.form->setColon(false);
    d.form->setRequiredMark(AdForm::RequiredMark::Hidden);
    d.form->setContentsMargins(0, 0, 12, 0);
    scroll->setContentWidget(d.form);
    // QScrollArea::setWidget enables background filling on the content widget.
    d.form->setAutoFillBackground(false);
    outer->addWidget(scroll);
    const auto segmented = [&d](const char* name, const QStringList& values) {
        auto* control = new AdSegmented(d.form);
        control->setObjectName(QString::fromLatin1(name));
        control->setDistribution(AdSegmented::Distribution::Fill);
        control->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        for (const auto& value : values)
            control->addOption({}, value);
        return control;
    };
    d.direction =
        segmented("imageSplitDirection", {QStringLiteral("vertical"), QStringLiteral("horizontal"),
                                          QStringLiteral("grid")});
    d.directionField = d.form->addField({}, d.direction, QStringLiteral("direction"));
    d.type = segmented("imageSplitType", {QStringLiteral("equal"), QStringLiteral("precise")});
    d.typeField = d.form->addField({}, d.type, QStringLiteral("type"));
    const auto number = [&d](const char* name) {
        auto* input = new AdInputNumber(d.form);
        input->setObjectName(QString::fromLatin1(name));
        // Retain fractional drafts so invalid integers receive inline feedback rather than
        // rounding.
        input->setDecimals(-1);
        input->setRangeMode(AdInputNumber::RangeMode::Permissive);
        input->setRange(1, std::numeric_limits<int>::max());
        input->setSingleStep(1);
        input->setKeyboardTracking(false);
        input->setWheelStepEnabled(false);
        input->setStepButtonLayout(AdInputNumber::StepButtonLayout::Compact);
        input->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        input->setLocale(QLocale::c());
        return input;
    };
    d.count = number("imageSplitCount");
    d.countField = d.form->addField({}, d.count, QStringLiteral("count"));
    d.rows = number("imageSplitRows");
    d.rowsField = d.form->addField({}, d.rows, QStringLiteral("rows"));
    d.columns = number("imageSplitColumns");
    d.columnsField = d.form->addField({}, d.columns, QStringLiteral("columns"));
    auto* precise = new QWidget(d.form);
    auto* preciseLayout = new QVBoxLayout(precise);
    preciseLayout->setContentsMargins(0, 0, 0, 0);
    preciseLayout->setSpacing(metric.marginXS);
    auto* preciseHeader = new QHBoxLayout;
    preciseHeader->setContentsMargins(0, 0, 0, 0);
    preciseHeader->setSpacing(metric.marginXS);
    d.sizeLabel = new QLabel(precise);
    d.sizeLabel->setWordWrap(true);
    preciseHeader->addWidget(d.sizeLabel, 1);
    d.unit = segmented("imageSplitUnit", {QStringLiteral("pixels"), QStringLiteral("percentage")});
    d.unit->setControlSize(AdSegmented::ControlSize::Small);
    d.unit->setDistribution(AdSegmented::Distribution::Content);
    preciseHeader->addWidget(d.unit, 0, Qt::AlignVCenter);
    preciseLayout->addLayout(preciseHeader);
    d.size = number("imageSplitSize");
    d.sizeLabel->setBuddy(d.size);
    preciseLayout->addWidget(d.size);
    d.sizeField = d.form->addField({}, precise, QStringLiteral("size"));
    d.manual = new QLabel(d.form);
    d.manual->setObjectName(QStringLiteral("imageSplitManualStatus"));
    d.manual->setWordWrap(true);
    auto* manualField = d.form->addField({}, d.manual);
    manualField->setNoStyle(true);
    // Split validation owns errors, including uncommitted drafts. Generic form validation would
    // clear them after a numeric value/hasValue signal without checking the split constraints.
    for (auto* field : d.form->items())
        field->setValidateOnChange(false);
    d.form->layout()->setAlignment(Qt::AlignTop);
    d.description = new QWidget(this);
    d.description->setObjectName(QStringLiteral("imageSplitOutputDescription"));
    d.description->hide();
    auto* descriptionLayout = new QHBoxLayout(d.description);
    descriptionLayout->setContentsMargins(0, 0, 0, 0);
    descriptionLayout->setSpacing(metric.marginSM);
    d.dimensions = new QLabel(d.description);
    d.dimensions->setObjectName(QStringLiteral("imageSplitDimensions"));
    d.total = new QLabel(d.description);
    d.total->setObjectName(QStringLiteral("imageSplitOutputCount"));
    descriptionLayout->addWidget(d.dimensions);
    descriptionLayout->addWidget(d.total);
    descriptionLayout->addStretch(1);
    d.dropHint = new QLabel(this);
    d.dropHint->setObjectName(QStringLiteral("imageSplitDropHint"));
    d.dropHint->setWordWrap(true);
    d.dropHint->setAlignment(Qt::AlignCenter);
    d.dropHint->setAutoFillBackground(true);
    d.dropHint->hide();
    const auto wireNumber = [this](AdInputNumber* input, const QString& key) {
        connect(input, &AdInputNumber::textChanged, this, [this, key](const QString& value) {
            auto& state = *m_impl;
            if (state.syncing)
                return;
            state.drafts.insert(key.isEmpty() ? state.sizeKey() : key, value);
            state.validate();
        });
        connect(input, &AdInputNumber::valueChanged, this, [this](double) { m_impl->commit(); });
        connect(input, &QAbstractSpinBox::editingFinished, this, [this] { m_impl->commit(); });
    };
    wireNumber(d.count, QStringLiteral("count"));
    wireNumber(d.rows, QStringLiteral("rows"));
    wireNumber(d.columns, QStringLiteral("columns"));
    wireNumber(d.size, {});
    connect(d.direction, &AdSegmented::currentValueChanged, this, [this](const QVariant& value) {
        auto& state = *m_impl;
        if (state.syncing || state.busy())
            return;
        Q_UNUSED(value);
        state.updateVisibility();
        state.commit();
    });
    connect(d.type, &AdSegmented::currentValueChanged, this, [this](const QVariant& value) {
        auto& state = *m_impl;
        if (state.syncing || state.busy())
            return;
        Q_UNUSED(value);
        state.updateVisibility();
        state.commit();
    });
    connect(d.unit, &AdSegmented::currentValueChanged, this,
            [this](const QVariant& value) { m_impl->changeUnit(value); });
    connect(d.actions[0], &AdButton::clicked, document, &ImageSplitDocument::undo);
    connect(d.actions[1], &AdButton::clicked, document, &ImageSplitDocument::redo);
    connect(d.actions[2], &AdButton::clicked, d.canvas, &ImageSplitPreviewCanvas::fitToCanvas);
    connect(d.actions[3], &AdButton::clicked, this, &ImageSplitEditorWidget::reloadRequested);
    connect(document, &ImageSplitDocument::changed, this, [this] { m_impl->refresh(); });
    connect(document, &ImageSplitDocument::historyChanged, this,
            [this] { m_impl->updateActions(); });
    auto* shortcuts = new WindowShortcutManager(this);
    shortcuts->addScopeWindow(this);
    for (const bool undo : {true, false}) {
        WindowShortcutManager::Binding binding;
        binding.id = undo ? QStringLiteral("imageSplitUndo") : QStringLiteral("imageSplitRedo");
        for (const auto& sequence :
             QKeySequence::keyBindings(undo ? QKeySequence::Undo : QKeySequence::Redo)) {
            if (sequence.count() == 1)
                binding.keyCombinations.append(sequence[0]);
        }
        binding.priority = WindowShortcutManager::StandardPriority::WindowCommand;
        binding.canActivate = [this, undo](const auto& context) {
            return !m_impl->busy() &&
                   !WindowShortcutManager::focusAcceptsTextInput(context.focusWidget) &&
                   (undo ? m_impl->document->canUndo() : m_impl->document->canRedo());
        };
        binding.activate = [this, undo](const auto&) {
            if (undo)
                m_impl->document->undo();
            else
                m_impl->document->redo();
            return true;
        };
        static_cast<void>(shortcuts->addBinding(this, std::move(binding)));
    }
    connect(&adqt::theme::ThemeManager::instance(), &adqt::theme::ThemeManager::themeChanged, this,
            [this] { m_impl->theme(); });
    connect(&styles::ThemeManager::instance(), &styles::ThemeManager::themeChanged, this,
            [this, outer, previewColumn, actions, descriptionLayout, preciseLayout,
             preciseHeader](const styles::ThemeColorScheme& scheme) {
                outer->setSpacing(scheme.metricAlias.marginMD);
                previewColumn->setSpacing(scheme.metricAlias.marginXS);
                actions->setSpacing(scheme.metricAlias.marginXS);
                descriptionLayout->setSpacing(scheme.metricAlias.marginSM);
                preciseLayout->setSpacing(scheme.metricAlias.marginXS);
                preciseHeader->setSpacing(scheme.metricAlias.marginXS);
                m_impl->theme();
            });
    d.sync();
    retranslateUi();
    d.refresh();
    d.theme();
}
ImageSplitEditorWidget::~ImageSplitEditorWidget() = default;
ImageSplitPreviewCanvas* ImageSplitEditorWidget::canvas() const {
    return m_impl->canvas;
}
bool ImageSplitEditorWidget::configurationValid() const {
    return m_impl->valid;
}
bool ImageSplitEditorWidget::commitPendingSettings() {
    if (m_impl->busy())
        return false;
    m_impl->commit();
    return m_impl->valid && m_impl->draftSettings().first == m_impl->document->settings();
}
QWidget* ImageSplitEditorWidget::outputDescriptionWidget() const {
    return m_impl->description;
}
void ImageSplitEditorWidget::setLoading(bool loading) {
    m_impl->loading = loading;
    m_impl->updateActions();
}
void ImageSplitEditorWidget::setExportBusy(bool busy) {
    m_impl->exporting = busy;
    m_impl->updateActions();
}
void ImageSplitEditorWidget::retranslateUi() {
    auto& d = *m_impl;
    d.validatedSettings.reset();
    const std::array<const char*, 3> directions{"Vertical", "Horizontal", "Grid"};
    for (std::size_t i = 0; i < directions.size(); ++i)
        d.direction->setOptionLabel(static_cast<int>(i), editorText(directions[i]));
    d.directionField->setLabel(editorText("Split direction"));
    d.direction->setAccessibleName(d.directionField->label());
    d.type->setOptionLabel(0, editorText("Equal Split"));
    d.type->setOptionLabel(1, editorText("Precise Split"));
    d.typeField->setLabel(editorText("Split type"));
    d.type->setAccessibleName(d.typeField->label());
    for (const auto& item : {std::pair{d.countField, d.count}, std::pair{d.rowsField, d.rows},
                             std::pair{d.columnsField, d.columns}}) {
        const auto source = item.first == d.countField  ? "Number of Images"
                            : item.first == d.rowsField ? "Rows"
                                                        : "Columns";
        item.first->setLabel(editorText(source));
        item.second->setAccessibleName(item.first->label());
    }
    d.unit->setOptionLabel(0, editorText("Pixels"));
    d.unit->setOptionLabel(1, editorText("Percentage"));
    d.unit->setAccessibleName(editorText("Split size unit"));
    d.size->setSuffixText(d.sizeKey() == QStringLiteral("pixels") ? editorText(" px")
                                                                  : QStringLiteral("%"));
    d.updateVisibility();
    const std::array<const char*, 4> actions{"Undo", "Redo", "Fit to Canvas", "Reload"};
    for (std::size_t i = 0; i < actions.size(); ++i) {
        d.actions[i]->setText(editorText(actions[i]));
        d.actions[i]->setAccessibleName(d.actions[i]->text());
    }
    d.manual->setText(editorText("Split lines adjusted manually"));
    d.dropHint->setText(editorText("Drop one image to replace the current image"));
    d.dropHint->setAccessibleName(d.dropHint->text());
    d.canvas->retranslateUi();
    d.refresh();
}
void ImageSplitEditorWidget::changeEvent(QEvent* event) {
    if (event->type() == QEvent::LanguageChange)
        retranslateUi();
    QWidget::changeEvent(event);
}
void ImageSplitEditorWidget::dragEnterEvent(QDragEnterEvent* event) {
    if (!m_impl->busy() && !droppedPaths(event->mimeData()).isEmpty() &&
        (event->possibleActions() & Qt::CopyAction)) {
        event->setDropAction(Qt::CopyAction);
        event->accept();
        m_impl->dropHint->setGeometry(m_impl->canvas->geometry());
        m_impl->dropHint->show();
        m_impl->dropHint->raise();
    }
}
void ImageSplitEditorWidget::dragMoveEvent(QDragMoveEvent* event) {
    if (!m_impl->busy() && !droppedPaths(event->mimeData()).isEmpty()) {
        event->setDropAction(Qt::CopyAction);
        event->accept();
    }
}
void ImageSplitEditorWidget::dragLeaveEvent(QDragLeaveEvent*) {
    m_impl->dropHint->hide();
}
void ImageSplitEditorWidget::dropEvent(QDropEvent* event) {
    m_impl->dropHint->hide();
    if (m_impl->busy())
        return;
    const auto paths = droppedPaths(event->mimeData());
    if (!paths.isEmpty()) {
        event->setDropAction(Qt::CopyAction);
        event->accept();
        emit imageFilesDropped(paths);
    }
}
void ImageSplitEditorWidget::hideEvent(QHideEvent* event) {
    m_impl->dropHint->hide();
    QWidget::hideEvent(event);
}

} // namespace snow_shot::presentation::components
