#include "snow_draw_engine_qt/snow_canvas_runtime.h"
#include "snow_draw_engine_qt/snow_canvas_custom_renderer.h"
#include "snow_draw_engine_qt/snow_canvas_widget.h"

#include <QApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMouseEvent>
#include <QPaintEvent>
#include <QPainter>
#include <QtMath>

#include <cmath>
#include <cstdlib>
#include <iostream>

namespace {
void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(EXIT_FAILURE);
    }
}

void mouse(SnowCanvasWidget& canvas, QEvent::Type type, QPointF point, Qt::MouseButton button,
           Qt::MouseButtons buttons) {
    QMouseEvent event(type, point, point, point, button, buttons, Qt::NoModifier);
    QApplication::sendEvent(&canvas, &event);
    QApplication::processEvents();
}

void drag(SnowCanvasWidget& canvas, QPointF start, QPointF end) {
    mouse(canvas, QEvent::MouseButtonPress, start, Qt::LeftButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseMove, end, Qt::NoButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseButtonRelease, end, Qt::LeftButton, Qt::NoButton);
}

QJsonObject magnifier(const SnowCanvasRuntime& runtime) {
    const auto document = QJsonDocument::fromJson(runtime.serializeDocumentSession())
                              .object()
                              .value(QStringLiteral("document"))
                              .toObject();
    QJsonObject result;
    int count = 0;
    for (const auto& slot : document.value(QStringLiteral("slots")).toArray()) {
        const auto data = slot.toObject().value(QStringLiteral("data")).toObject();
        if (data.contains(QStringLiteral("Magnifier"))) {
            result = data.value(QStringLiteral("Magnifier")).toObject();
            ++count;
        }
    }
    require(count == 1, "a drag creates one compound magnifier annotation");
    return result;
}

QPointF center(const QJsonObject& value, const QString& field) {
    const auto point = value.value(field).toObject();
    return {point.value(QStringLiteral("x")).toDouble(),
            point.value(QStringLiteral("y")).toDouble()};
}

bool near(QPointF a, QPointF b) {
    return std::hypot(a.x() - b.x(), a.y() - b.y()) < 0.001;
}

void prepare(SnowCanvasWidget& canvas) {
    canvas.resize(800, 600);
    canvas.show();
    QApplication::processEvents();
    require(canvas.setCanvasTool(SnowCanvasTool::Magnifier), "activate Magnifier");
}

void compoundDraggingAndDefaultIsolation() {
    for (const auto shape : {SnowCanvasRectangleShape::Rectangle, SnowCanvasRectangleShape::Ellipse,
                             SnowCanvasRectangleShape::Diamond}) {
        SnowCanvasRuntime runtime;
        SnowCanvasWidget canvas(runtime);
        prepare(canvas);
        auto style = canvas.canvasMagnifierStyle();
        style.shape = shape;
        require(canvas.setCanvasMagnifierStyle(style), "set magnifier creation shape");
        drag(canvas, {220, 160}, {300, 220});
        const auto created = magnifier(runtime);
        const auto source = center(created, QStringLiteral("source_center"));
        const auto lens = center(created, QStringLiteral("magnified_center"));
        require(near(source, lens) && created.value(QStringLiteral("width")).toDouble() == 80 &&
                    created.value(QStringLiteral("height")).toDouble() == 60 &&
                    created.value(QStringLiteral("factor")).toDouble() == 2,
                "creation retains the entire source and initially aligns both regions");

        const QPointF delta(30, 15);
        const auto sourceView = canvas.canvasToViewTransform().map(source);
        drag(canvas, sourceView, sourceView + delta);
        const auto moved = magnifier(runtime);
        require(near(center(moved, QStringLiteral("source_center")), source + delta) &&
                    near(center(moved, QStringLiteral("magnified_center")), lens + delta),
                "the source wins overlap hit testing and moves both regions");
        require(canvas.undo() && magnifier(runtime) == created,
                "one undo restores the complete source drag");
        require(canvas.redo(), "redo restores the source drag");

        const auto movedLens = center(moved, QStringLiteral("magnified_center"));
        const auto lensView = canvas.canvasToViewTransform().map(movedLens + QPointF(60, 0));
        drag(canvas, lensView, lensView + QPointF(180, 0));
        const auto separated = magnifier(runtime);
        require(near(center(separated, QStringLiteral("source_center")), source + delta) &&
                    near(center(separated, QStringLiteral("magnified_center")),
                         movedLens + QPointF(180, 0)),
                "dragging the exposed magnified region preserves the source");

        const auto resizeHandle =
            canvas.canvasToViewTransform().map(source + delta + QPointF(40, 30));
        drag(canvas, resizeHandle, resizeHandle + QPointF(20, 10));
        const auto resized = magnifier(runtime);
        require(resized.value(QStringLiteral("width")).toDouble() == 100 &&
                    resized.value(QStringLiteral("height")).toDouble() == 70 &&
                    near(center(resized, QStringLiteral("magnified_center")),
                         center(separated, QStringLiteral("magnified_center"))) &&
                    resized.value(QStringLiteral("factor")).toDouble() == 2,
                "source resize handles preserve the magnified center and factor");
        require(canvas.undo() && magnifier(runtime) == separated,
                "source resizing is independently undoable");

        style.factor = 6;
        require(canvas.applyStyleEdit(
                    SnowCanvasMagnifierEdit{style, SnowCanvasMagnifierStylePropertyFactor, true}),
                "creation defaults can be replicated while an element is selected");
        require(magnifier(runtime).value(QStringLiteral("factor")).toDouble() == 2,
                "default replication cannot mutate a selected magnifier");
        style.factor = 3;
        require(canvas.commitStyleEdit(
                    SnowCanvasMagnifierEdit{style, SnowCanvasMagnifierStylePropertyFactor, false}),
                "selected magnification can be edited");
        require(magnifier(runtime).value(QStringLiteral("factor")).toDouble() == 3 &&
                    canvas.undo() && magnifier(runtime) == separated,
                "selected styles are undoable independently of creation defaults");
    }
}

void coincidentOneTimesLensGrip() {
    SnowCanvasRuntime runtime;
    SnowCanvasWidget canvas(runtime);
    prepare(canvas);
    auto style = canvas.canvasMagnifierStyle();
    style.factor = 1;
    require(canvas.setCanvasMagnifierStyle(style), "set one-times creation default");
    drag(canvas, {220, 160}, {300, 220});
    const auto original = magnifier(runtime);
    const auto source = center(original, QStringLiteral("source_center"));
    const auto sourceView = canvas.canvasToViewTransform().map(source);
    mouse(canvas, QEvent::MouseButtonPress, sourceView, Qt::LeftButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseButtonRelease, sourceView, Qt::LeftButton, Qt::NoButton);
    const auto grip = canvas.canvasToViewTransform().map(source + QPointF(0, 54));
    drag(canvas, grip, grip + QPointF(150, 0));
    const auto moved = magnifier(runtime);
    require(near(center(moved, QStringLiteral("source_center")), source) &&
                near(center(moved, QStringLiteral("magnified_center")), source + QPointF(150, 0)),
            "the external move grip separates a coincident one-times lens");
    require(canvas.undo() && magnifier(runtime) == original,
            "lens grip dragging remains a single undoable operation");
}

class DamageTrackingCanvas final : public SnowCanvasWidget {
  public:
    QRegion painted;

  protected:
    void paintEvent(QPaintEvent* event) override {
        painted += event->region();
        SnowCanvasWidget::paintEvent(event);
    }
};

class MutableImageRenderer final : public SnowCanvasCustomRenderer {
  public:
    QImage image;
    QRectF canvasRect;
    bool sourcesEnabled = true;
    std::uint64_t revision = 1;

    explicit MutableImageRenderer(QSize size) : image(size, QImage::Format_ARGB32_Premultiplied) {
        image.fill(Qt::blue);
    }

    std::uint64_t contentRevision() const override {
        return revision;
    }

    QList<SnowCanvasBaseImageSource> baseImageSources() const override {
        return sourcesEnabled ? QList<SnowCanvasBaseImageSource>{{image, canvasRect, {}}}
                              : QList<SnowCanvasBaseImageSource>{};
    }

    void renderBeforeCanvas(QPainter& painter, const SnowCanvasRenderContext& context) override {
        painter.save();
        painter.setWorldTransform(context.canvasToViewTransform, true);
        painter.drawImage(canvasRect, image);
        painter.restore();
    }
};

void prepareDamageCanvas(DamageTrackingCanvas& canvas, MutableImageRenderer& renderer,
                         double zoom = 1.0) {
    canvas.resize(400, 240);
    canvas.show();
    QApplication::processEvents();
    require(canvas.setViewportCamera(200, 120, zoom), "configure partial-image damage camera");
    renderer.canvasRect = canvas.canvasToViewTransform().inverted().mapRect(QRectF(canvas.rect()));
    canvas.setCustomRenderer(&renderer);
    QApplication::processEvents();
}

void addSeparatedLens(SnowCanvasWidget& canvas, const QRectF& source, const QPointF& delta) {
    require(canvas.setCanvasTool(SnowCanvasTool::Magnifier), "activate the damage-test lens");
    auto style = canvas.canvasMagnifierStyle();
    style.factor = 2;
    style.strokeWidth = 0;
    style.cornerRadii = {};
    style.showLeader = false;
    require(canvas.setCanvasMagnifierStyle(style), "configure the damage-test lens");
    drag(canvas, source.topLeft(), source.bottomRight());
    mouse(canvas, QEvent::MouseButtonPress, source.center(), Qt::LeftButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseButtonRelease, source.center(), Qt::LeftButton, Qt::NoButton);
    const QPointF grip = source.center() + QPointF(0, source.height() + 24);
    drag(canvas, grip, grip + delta);
    require(canvas.resetEditingState(), "finish the separated damage-test lens");
}

void addBlur(SnowCanvasWidget& canvas, const QRectF& bounds) {
    require(canvas.setCanvasTool(SnowCanvasTool::RectangleFilter), "activate damage-test blur");
    require(canvas.setCanvasFilterStyle({SnowCanvasFilterType::GaussianBlur, 1, 1, 2},
                                        SnowCanvasFilterStylePropertyType |
                                            SnowCanvasFilterStylePropertyStrength),
            "configure damage-test blur support");
    drag(canvas, bounds.topLeft(), bounds.bottomRight());
    require(canvas.resetEditingState(), "finish the damage-test blur");
}

QImage fullRender(SnowCanvasWidget& canvas) {
    const qreal dpr = canvas.devicePixelRatioF();
    QImage output(QSize(qCeil(canvas.width() * dpr), qCeil(canvas.height() * dpr)),
                  QImage::Format_ARGB32_Premultiplied);
    output.setDevicePixelRatio(dpr);
    output.fill(Qt::transparent);
    QPainter painter(&output);
    canvas.render(&painter);
    return output;
}

int changedPixels(const QImage& before, const QImage& after, const QRect& bounds) {
    int changed = 0;
    const qreal dpr = before.devicePixelRatio();
    for (int y = 0; y < before.height(); ++y) {
        for (int x = 0; x < before.width(); ++x) {
            if (bounds.contains(QPoint(qFloor(x / dpr), qFloor(y / dpr))) &&
                before.pixel(x, y) != after.pixel(x, y))
                ++changed;
        }
    }
    return changed;
}

void requireChangedPixelsRepainted(const QImage& before, const QImage& after,
                                   const QRegion& painted) {
    int changed = 0;
    const qreal dpr = before.devicePixelRatio();
    for (int y = 0; y < before.height(); ++y) {
        for (int x = 0; x < before.width(); ++x) {
            if (before.pixel(x, y) == after.pixel(x, y))
                continue;
            ++changed;
            require(painted.contains(QPoint(qFloor(x / dpr), qFloor(y / dpr))),
                    "partial original-image damage must repaint every changed output pixel");
        }
    }
    require(changed > 0, "the partial original-image change must affect rendered pixels");
}

void originalDamageClosesThroughLaterFilterPasses() {
    for (bool chained : {false, true}) {
        MutableImageRenderer renderer(QSize(400, 240));
        DamageTrackingCanvas canvas;
        prepareDamageCanvas(canvas, renderer);
        addSeparatedLens(canvas, QRectF(20, 80, 30, 30), QPointF(95, 0));
        addBlur(canvas, QRectF(90, 45, 100, 110));
        if (chained) {
            // This lens separates filter source passes, but its pristine source is unchanged.
            addSeparatedLens(canvas, QRectF(170, 80, 30, 30), QPointF(140, 0));
            addBlur(canvas, QRectF(205, 45, 70, 110));
        }
        const QImage before = fullRender(canvas);
        QApplication::processEvents();
        canvas.painted = {};
        {
            QPainter painter(&renderer.image);
            painter.fillRect(QRect(20, 80, 30, 30), Qt::red);
        }
        ++renderer.revision;
        canvas.updateBaseImageContent(QRegion(QRect(20, 80, 30, 30)));
        QApplication::processEvents();
        const QRegion painted = canvas.painted;
        const QImage after = fullRender(canvas);
        require(changedPixels(before, after, QRect(163, 65, 25, 70)) > 0,
                "the blur must propagate changed lens pixels outside the lens bounds");
        if (chained) {
            require(changedPixels(before, after, QRect(210, 65, 30, 70)) > 0,
                    "a later filter pass must sample the changed earlier blur output");
            require(!painted.contains(QPoint(325, 95)),
                    "expanded filter damage cannot invalidate a lens sampling unchanged originals");
        }
        requireChangedPixelsRepainted(before, after, painted);
        canvas.setCustomRenderer(nullptr);
    }
}

void siblingFiltersKeepTheirSharedEnteringDamage() {
    MutableImageRenderer renderer(QSize(400, 240));
    DamageTrackingCanvas canvas;
    prepareDamageCanvas(canvas, renderer);
    addSeparatedLens(canvas, QRectF(20, 80, 30, 30), QPointF(95, 0));
    addBlur(canvas, QRectF(140, 75, 80, 40));
    addBlur(canvas, QRectF(250, 75, 30, 40));
    const QImage before = fullRender(canvas);
    QApplication::processEvents();
    canvas.painted = {};
    {
        QPainter painter(&renderer.image);
        painter.fillRect(QRect(20, 80, 30, 30), Qt::red);
    }
    ++renderer.revision;
    canvas.updateBaseImageContent(QRegion(QRect(20, 80, 30, 30)));
    QApplication::processEvents();
    const QRegion painted = canvas.painted;
    const QImage after = fullRender(canvas);
    require(changedPixels(before, after, QRect(170, 80, 40, 30)) > 0,
            "the first sibling filter must respond to changed lens pixels");
    require(changedPixels(before, after, QRect(250, 75, 30, 40)) == 0 &&
                !painted.contains(QPoint(265, 95)),
            "sibling filters cannot consume or propagate each other's changed output");
    requireChangedPixelsRepainted(before, after, painted);
    canvas.setCustomRenderer(nullptr);
}

void sourcePixelSupportSurvivesZoomAndSourceReplacement() {
    for (double zoom : {1.0, 2.0}) {
        for (int change = 0; change < 3; ++change) {
            MutableImageRenderer renderer(QSize(40, 24));
            renderer.image.setPixelColor(5, 8, Qt::green);
            DamageTrackingCanvas canvas;
            prepareDamageCanvas(canvas, renderer, zoom);
            addSeparatedLens(canvas, QRectF(63.5, 80, 30, 30), QPointF(185, 0));
            const QImage before = fullRender(canvas);
            QApplication::processEvents();
            canvas.painted = {};
            if (change == 0) {
                renderer.image.setPixelColor(5, 8, Qt::red);
            } else if (change == 1) {
                renderer.image = QImage(QSize(400, 240), QImage::Format_ARGB32_Premultiplied);
                renderer.image.fill(Qt::blue);
            } else {
                renderer.sourcesEnabled = false;
            }
            ++renderer.revision;
            canvas.updateBaseImageContent(QRegion(QRect(50, 80, 10, 10)));
            QApplication::processEvents();
            const QRegion painted = canvas.painted;
            const QImage after = fullRender(canvas);
            require(
                changedPixels(before, after, QRect(234, 65, 61, 60)) > 0,
                "an original pixel outside the source outline contributes to lens interpolation");
            requireChangedPixelsRepainted(before, after, painted);
            canvas.setCustomRenderer(nullptr);
        }
    }
}
} // namespace

int main(int argc, char** argv) {
    qputenv("QT_QPA_PLATFORM", QByteArrayLiteral("offscreen"));
    QApplication app(argc, argv);
    compoundDraggingAndDefaultIsolation();
    coincidentOneTimesLensGrip();
    originalDamageClosesThroughLaterFilterPasses();
    siblingFiltersKeepTheirSharedEnteringDamage();
    sourcePixelSupportSurvivesZoomAndSourceReplacement();
    return EXIT_SUCCESS;
}
