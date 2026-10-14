// SPDX-License-Identifier: GPL-3.0-only
#include "snow_shot/presentation/components/imagespliteditorwidget.h"

#include "theme/theme_manager.h"
#include "widgets/button.h"
#include "widgets/form.h"
#include "widgets/input_number.h"
#include "widgets/segmented.h"

#include <QApplication>
#include <QDragEnterEvent>
#include <QDropEvent>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMimeData>
#include <QMouseEvent>
#include <QPainter>
#include <QTranslator>
#include <QUrl>
#include <QWheelEvent>

#include <array>
#include <cmath>
#include <iostream>
#include <stdexcept>

namespace {
using namespace snow_shot::presentation;
using namespace snow_shot::presentation::components;
using adqt::widgets::AdButton;
using adqt::widgets::AdForm;
using adqt::widgets::AdInputNumber;
using adqt::widgets::AdSegmented;

void require(bool condition, const char* message) {
    if (!condition)
        throw std::runtime_error(message);
}
QImage image(QSize size) {
    QImage result(size, QImage::Format_ARGB32_Premultiplied);
    result.fill(QColor(20, 80, 160, 100));
    return result;
}
void pump() {
    QApplication::processEvents();
}
void mouse(QWidget* target, QEvent::Type type, QPointF point, Qt::MouseButton button,
           Qt::MouseButtons buttons) {
    QMouseEvent event(type, point, target->mapToGlobal(point.toPoint()), button, buttons,
                      Qt::NoModifier);
    QApplication::sendEvent(target, &event);
}
void click(QWidget* target, QPointF point) {
    mouse(target, QEvent::MouseButtonPress, point, Qt::LeftButton, Qt::LeftButton);
    mouse(target, QEvent::MouseButtonRelease, point, Qt::LeftButton, Qt::NoButton);
    pump();
}
void click(QWidget* target) {
    click(target, QRectF(target->rect()).center());
}
void wheel(QWidget* target, QPointF point, int delta) {
    QWheelEvent event(point, target->mapToGlobal(point.toPoint()), {}, QPoint(0, delta),
                      Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(target, &event);
}
template <class T> T* child(QWidget& editor, const char* name) {
    auto* result = editor.findChild<T*>(QString::fromLatin1(name));
    require(result != nullptr, "expected split editor control is missing");
    return result;
}
void show(ImageSplitEditorWidget& editor) {
    editor.resize(1100, 720);
    editor.show();
    pump();
}

void defaultsConditionalControlsAndActions() {
    ImageSplitDocument document;
    document.setImage(image({1200, 900}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    auto* form = child<AdForm>(editor, "imageSplitSettingsForm");
    require(
        !form->autoFillBackground(),
        "the settings form must inherit its modal background after attaching to the scroll area");
    auto* direction = child<AdSegmented>(editor, "imageSplitDirection");
    auto* type = child<AdSegmented>(editor, "imageSplitType");
    auto* count = child<AdInputNumber>(editor, "imageSplitCount");
    require(direction->currentValue() == QStringLiteral("vertical") &&
                type->currentValue() == QStringLiteral("equal") && count->value() == 2 &&
                editor.configurationValid() && document.layout().outputCount() == 2,
            "opening an image must show vertical equal splitting into two images");
    require(!form->field(QStringLiteral("type"))->itemHidden() &&
                !form->field(QStringLiteral("count"))->itemHidden() &&
                form->field(QStringLiteral("rows"))->itemHidden() &&
                form->field(QStringLiteral("size"))->itemHidden(),
            "equal splitting must show only its applicable controls");
    direction->setCurrentValue(QStringLiteral("grid"));
    require(form->field(QStringLiteral("type"))->itemHidden() &&
                form->field(QStringLiteral("count"))->itemHidden() &&
                !form->field(QStringLiteral("rows"))->itemHidden() &&
                !form->field(QStringLiteral("columns"))->itemHidden() &&
                document.layout().outputCount() == 4,
            "grid must hide split type and display the default two by two controls");
    child<AdInputNumber>(editor, "imageSplitColumns")->setValue(3);
    require(document.settings().rows == 2 && document.settings().columns == 3 &&
                document.layout().outputCount() == 6 &&
                document.layout().regionAt(3).top() == 450 &&
                document.layout().regionAt(3).left() == 0,
            "a rectangular grid must retain row-major output ordering");
    direction->setCurrentValue(QStringLiteral("horizontal"));
    type->setCurrentValue(QStringLiteral("precise"));
    require(!form->field(QStringLiteral("size"))->itemHidden() &&
                form->field(QStringLiteral("count"))->itemHidden() &&
                child<AdInputNumber>(editor, "imageSplitSize")->exactValue() ==
                    QStringLiteral("50") &&
                child<AdInputNumber>(editor, "imageSplitSize")->accessibleName() ==
                    QStringLiteral("Height per Image"),
            "horizontal precise splitting must show a height and default fifty percent");
    const std::array<const char*, 4> names{"imageSplitUndo", "imageSplitRedo", "imageSplitFit",
                                           "imageSplitReload"};
    const std::array<const char*, 4> labels{"Undo", "Redo", "Fit to Canvas", "Reload"};
    int lastX = -1;
    for (std::size_t i = 0; i < names.size(); ++i) {
        auto* button = child<AdButton>(editor, names[i]);
        require(button->buttonStyle() == AdButton::ButtonStyle::Text &&
                    button->text() == QString::fromLatin1(labels[i]) &&
                    button->y() >= editor.canvas()->geometry().bottom() && button->x() > lastX,
                "canvas actions must be ordered text buttons below the canvas");
        lastX = button->x();
    }
}

void formValidationUnitsAndUndoableRegeneration() {
    ImageSplitDocument document;
    document.setImage(image({1001, 800}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    auto* count = child<AdInputNumber>(editor, "imageSplitCount");
    auto* form = child<AdForm>(editor, "imageSplitSettingsForm");
    count->clear();
    require(!editor.configurationValid() &&
                !form->field(QStringLiteral("count"))->errorMessages().isEmpty() &&
                document.settings().count == 2,
            "empty draft input must show an immediate inline error and preserve document settings");
    count->setValue(2.5);
    require(!editor.configurationValid() && document.settings().count == 2,
            "fractional image counts must be rejected instead of rounded");
    count->setValue(3);
    require(editor.configurationValid() && document.settings().count == 3,
            "correcting a draft must restore validity and commit the new count");
    document.createLine(true, 100);
    const auto manual = document.state();
    QMetaObject::invokeMethod(count, "editingFinished", Qt::DirectConnection);
    require(document.state() == manual,
            "finishing an unchanged numeric field must preserve manual split lines");
    count->setValue(4);
    require(!document.state().manuallyEdited && document.layout().outputCount() == 4,
            "a parameter change must replace all manually edited boundaries");
    click(child<AdButton>(editor, "imageSplitUndo"));
    require(document.state() == manual &&
                child<AdInputNumber>(editor, "imageSplitCount")->value() == 3,
            "undo must restore both manual lines and their form settings");
    click(child<AdButton>(editor, "imageSplitRedo"));
    require(document.settings().count == 4 && !document.state().manuallyEdited,
            "redo must restore parameter-generated boundaries");
    child<AdSegmented>(editor, "imageSplitType")->setCurrentValue(QStringLiteral("precise"));
    auto* size = child<AdInputNumber>(editor, "imageSplitSize");
    size->setExactValue(QStringLiteral("25"));
    const auto percentageLines = document.layout().verticalLines;
    require(percentageLines.size() == 3 && percentageLines.at(0) == 250 &&
                percentageLines.at(1) == 501 && percentageLines.at(2) == 751,
            "percentage edits must regenerate boundaries at rounded successive percentages");
    child<AdSegmented>(editor, "imageSplitUnit")->setCurrentValue(QStringLiteral("pixels"));
    require(document.settings().pixels == 250 && document.layout().outputCount() == 5,
            "switching to pixels must convert with half-up rounding and retain a remainder");
    size->setValue(300);
    child<AdSegmented>(editor, "imageSplitUnit")->setCurrentValue(QStringLiteral("percentage"));
    require(document.settings().percentage == QStringLiteral("29.97"),
            "switching to percentage must retain two decimal places");
    size->setExactValue(QStringLiteral("4.99"));
    require(!editor.configurationValid() &&
                document.settings().percentage == QStringLiteral("29.97"),
            "percentages below the chosen five-percent minimum must be invalid drafts");
    size->setExactValue(QStringLiteral("100"));
    require(editor.configurationValid() && document.layout().outputCount() == 1,
            "one hundred percent must produce exactly one output");
}

void unitConversionDoesNotClampInvalidResults() {
    ImageSplitDocument document;
    document.setImage(image({20, 10}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    child<AdSegmented>(editor, "imageSplitType")->setCurrentValue(QStringLiteral("precise"));
    auto* unit = child<AdSegmented>(editor, "imageSplitUnit");
    auto* size = child<AdInputNumber>(editor, "imageSplitSize");
    const auto beforeSmallConversion = document.state();
    unit->setCurrentValue(QStringLiteral("pixels"));
    require(unit->currentValue() == QStringLiteral("pixels") && size->value() == 10 &&
                !editor.configurationValid() && document.state() == beforeSmallConversion,
            "a pixel conversion below sixteen must remain visible and invalid without clamping or "
            "losing geometry");
    size->setValue(16);
    require(editor.configurationValid() && document.settings().unit == ImageSplitUnit::Pixels,
            "correcting a converted draft must commit its unit and size together");
    document.undo();
    require(document.state() == beforeSmallConversion &&
                unit->currentValue() == QStringLiteral("percentage"),
            "undoing a corrected unit conversion must restore the original unit and geometry");
    document.setImage(image({100, 80}));
    child<AdSegmented>(editor, "imageSplitType")->setCurrentValue(QStringLiteral("precise"));
    unit->setCurrentValue(QStringLiteral("pixels"));
    size->setValue(300);
    const auto beforeLargeConversion = document.state();
    unit->setCurrentValue(QStringLiteral("percentage"));
    require(size->exactValue() == QStringLiteral("300") && !editor.configurationValid() &&
                document.state() == beforeLargeConversion,
            "a pixel interval larger than the image must convert above one hundred percent without "
            "clamping");
    unit->setCurrentValue(QStringLiteral("pixels"));
    require(size->value() == 300 && editor.configurationValid() &&
                document.state() == beforeLargeConversion,
            "switching back from an invalid percentage must recover the unchanged pixel interval");
    document.setImage(image({1001, 800}));
    child<AdSegmented>(editor, "imageSplitType")->setCurrentValue(QStringLiteral("precise"));
    unit->setCurrentValue(QStringLiteral("pixels"));
    require(document.settings().pixels == 501,
            "percentage-to-pixel conversion must round exact half pixels upward");
}

void percentageDraftsKeepExactEnteredPrecision() {
    ImageSplitDocument document;
    document.setImage(image({1200, 900}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    child<AdSegmented>(editor, "imageSplitType")->setCurrentValue(QStringLiteral("precise"));
    auto* size = child<AdInputNumber>(editor, "imageSplitSize");
    auto* unit = child<AdSegmented>(editor, "imageSplitUnit");
    size->setExactValue(QStringLiteral("5.125"));
    require(editor.configurationValid() &&
                document.settings().percentage == QStringLiteral("5.125") &&
                document.layout().verticalLines.first() == 62,
            "entered percentages beyond two decimal places must remain exact and valid");
    unit->setCurrentValue(QStringLiteral("pixels"));
    require(document.settings().pixels == 62 && editor.configurationValid(),
            "exact percentage conversion must round half pixels upward");
    unit->setCurrentValue(QStringLiteral("percentage"));
    require(document.settings().percentage == QStringLiteral("5.17"),
            "pixel-to-percentage conversion must still display two decimal places");
    const auto belowHalf = QStringLiteral("5.1249999999999999999999999999999999999999999999999");
    size->setExactValue(belowHalf);
    require(editor.configurationValid() && document.settings().percentage == belowHalf &&
                document.layout().verticalLines.first() == 61,
            "a decimal just below a half pixel must not be rounded through floating point");
    unit->setCurrentValue(QStringLiteral("pixels"));
    require(document.settings().pixels == 61 && editor.configurationValid(),
            "unit conversion must use all entered percentage digits when rounding boundaries");
}

void transformedRulersAndManualGestures() {
    ImageSplitDocument document;
    document.setImage(image({1200, 900}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    auto* canvas = editor.canvas();
    const auto source = document.image();
    const QPointF anchor{250, 220};
    const auto anchoredPixel = canvas->viewportToImage(anchor);
    wheel(canvas, anchor, 240);
    require((canvas->viewportToImage(anchor) - anchoredPixel).manhattanLength() < .001,
            "wheel zoom must preserve the image coordinate under the pointer");
    mouse(canvas, QEvent::MouseButtonPress, {200, 150}, Qt::MiddleButton, Qt::MiddleButton);
    mouse(canvas, QEvent::MouseMove, {220, 180}, Qt::NoButton, Qt::MiddleButton);
    mouse(canvas, QEvent::MouseButtonRelease, {220, 180}, Qt::MiddleButton, Qt::NoButton);
    const auto rulerX = canvas->imageToViewport({300, 0}).x();
    click(canvas, {rulerX, 12});
    require(document.layout().verticalLines.contains(300),
            "top ruler clicks must create image-pixel coordinates synchronized with zoom and pan");
    const auto beforeBlocked = document.state();
    click(canvas, {12, canvas->imageToViewport({0, 200}).y()});
    require(document.state() == beforeBlocked,
            "the left ruler must not create horizontal lines in vertical mode");
    const auto start = canvas->imageToViewport({300, 400});
    mouse(canvas, QEvent::MouseButtonPress, start, Qt::LeftButton, Qt::LeftButton);
    for (int coordinate : {310, 320, 350})
        mouse(canvas, QEvent::MouseMove, canvas->imageToViewport({qreal(coordinate), 400}),
              Qt::NoButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseButtonRelease, canvas->imageToViewport({350, 400}), Qt::LeftButton,
          Qt::NoButton);
    require(document.layout().verticalLines.contains(350),
            "dragging must move the selected split line");
    document.undo();
    require(document.layout().verticalLines.contains(300),
            "one undo must reverse the entire drag gesture");
    document.redo();
    const auto beforeCancel = document.state();
    mouse(canvas, QEvent::MouseButtonPress, canvas->imageToViewport({350, 400}), Qt::LeftButton,
          Qt::LeftButton);
    mouse(canvas, QEvent::MouseMove, canvas->imageToViewport({380, 400}), Qt::NoButton,
          Qt::LeftButton);
    QKeyEvent escape(QEvent::KeyPress, Qt::Key_Escape, Qt::NoModifier);
    QApplication::sendEvent(canvas, &escape);
    mouse(canvas, QEvent::MouseButtonRelease, canvas->imageToViewport({380, 400}), Qt::LeftButton,
          Qt::NoButton);
    require(document.state() == beforeCancel,
            "Escape must restore a provisional drag without history");
    click(canvas, canvas->imageToViewport({350, 400}));
    QKeyEvent deletion(QEvent::KeyPress, Qt::Key_Delete, Qt::NoModifier);
    QApplication::sendEvent(canvas, &deletion);
    require(!document.layout().verticalLines.contains(350), "Delete must remove the selected line");
    document.undo();
    require(document.layout().verticalLines.contains(350), "line deletion must be undoable");
    const auto viewportZoom = canvas->zoom();
    const auto viewportOffset = canvas->offset();
    auto settings = document.settings();
    settings.count = 4;
    document.setSettings(settings);
    require(canvas->zoom() == viewportZoom && canvas->offset() == viewportOffset,
            "parameter regeneration must preserve the current viewport");
    canvas->fitToCanvas();
    const auto countBeforeViewport = document.state();
    wheel(canvas, {200, 150}, -120);
    canvas->fitToCanvas();
    require(document.state() == countBeforeViewport && document.image() == source,
            "viewport operations must preserve document state and source pixels");
    child<AdSegmented>(editor, "imageSplitDirection")->setCurrentValue(QStringLiteral("grid"));
    click(canvas, {12, canvas->imageToViewport({0, 200}).y()});
    require(document.layout().horizontalLines.contains(200),
            "grid must permit horizontal ruler edits");
}

void replacementResetBusyAndDrops() {
    ImageSplitDocument document;
    document.setImage(image({1200, 900}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    document.createLine(true, 300);
    child<AdInputNumber>(editor, "imageSplitCount")->clear();
    document.setImage(image({800, 600}));
    require(editor.configurationValid() && !document.canUndo() && !document.canRedo() &&
                child<AdInputNumber>(editor, "imageSplitCount")->value() == 2 &&
                document.layout().outputCount() == 2 &&
                !child<AdButton>(editor, "imageSplitUndo")->isEnabled() &&
                !child<AdButton>(editor, "imageSplitRedo")->isEnabled(),
            "a successful image replacement must reset form drafts and history");
    int reloads = 0;
    QObject::connect(&editor, &ImageSplitEditorWidget::reloadRequested, &editor,
                     [&] { ++reloads; });
    click(child<AdButton>(editor, "imageSplitReload"));
    require(reloads == 1, "Reload must request file selection");
    QStringList received;
    QObject::connect(&editor, &ImageSplitEditorWidget::imageFilesDropped, &editor,
                     [&](const QStringList& paths) { received = paths; });
    QMimeData multiple;
    multiple.setUrls({QUrl::fromLocalFile(QStringLiteral("C:/split-one.png")),
                      QUrl::fromLocalFile(QStringLiteral("C:/split-two.png"))});
    const auto beforeDrop = document.state();
    QDragEnterEvent enter(QPointF(100, 100), Qt::CopyAction, &multiple, Qt::LeftButton,
                          Qt::NoModifier);
    QApplication::sendEvent(&editor, &enter);
    QDropEvent drop({100, 100}, Qt::CopyAction, &multiple, Qt::LeftButton, Qt::NoModifier);
    QApplication::sendEvent(&editor, &drop);
    require(received.size() == 2 && document.state() == beforeDrop,
            "multi-file drops must preserve all files for owner validation without changing edits");
    received.clear();
    QMimeData mixed;
    mixed.setUrls({QUrl::fromLocalFile(QStringLiteral("C:/split-one.png")),
                   QUrl(QStringLiteral("https://example.com/image.png"))});
    QDragEnterEvent rejected(QPointF(100, 100), Qt::CopyAction, &mixed, Qt::LeftButton,
                             Qt::NoModifier);
    QApplication::sendEvent(&editor, &rejected);
    require(!rejected.isAccepted() && received.isEmpty(),
            "a mixed local and remote URL payload must never be filtered to one replacement");
    editor.setExportBusy(true);
    require(!child<AdButton>(editor, "imageSplitReload")->isEnabled() &&
                !child<AdSegmented>(editor, "imageSplitDirection")->isEnabled(),
            "busy export must prevent duplicate reloads and form changes");
    QDragEnterEvent busyEnter(QPointF(100, 100), Qt::CopyAction, &multiple, Qt::LeftButton,
                              Qt::NoModifier);
    QApplication::sendEvent(&editor, &busyEnter);
    require(!busyEnter.isAccepted(), "busy work must reject incoming image drops");
    editor.setExportBusy(false);
    require(child<AdButton>(editor, "imageSplitReload")->isEnabled(),
            "ending busy work must restore controls");
}

void invalidDraftsSurviveFocusAndCanceledLoading() {
    ImageSplitDocument document;
    document.setImage(image({1001, 800}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    auto* count = child<AdInputNumber>(editor, "imageSplitCount");
    auto* form = child<AdForm>(editor, "imageSplitSettingsForm");
    count->setFocus();
    count->setValue(0);
    const auto countState = document.state();
    editor.canvas()->setFocus();
    pump();
    editor.setLoading(true);
    editor.setLoading(false);
    require(count->displayText() == QStringLiteral("0") && !editor.configurationValid() &&
                !form->field(QStringLiteral("count"))->errorMessages().isEmpty() &&
                document.state() == countState,
            "an invalid zero count must survive focus loss and canceled replacement loading");
    child<AdSegmented>(editor, "imageSplitType")->setCurrentValue(QStringLiteral("precise"));
    auto* size = child<AdInputNumber>(editor, "imageSplitSize");
    document.createLine(true, 100);
    const auto percentageState = document.state();
    size->setFocus();
    size->setExactValue(QStringLiteral("4.99"));
    editor.canvas()->setFocus();
    pump();
    editor.setLoading(true);
    editor.setLoading(false);
    require(size->displayText() == QStringLiteral("4.99") && !editor.configurationValid() &&
                !form->field(QStringLiteral("size"))->errorMessages().isEmpty() &&
                document.state() == percentageState && document.canUndo(),
            "an invalid precise percentage must retain its exact text, errors, geometry and "
            "history after canceled loading");
    child<AdSegmented>(editor, "imageSplitUnit")->setCurrentValue(QStringLiteral("pixels"));
    require(size->value() == 50 && editor.configurationValid() && document.settings().pixels == 50,
            "changing units must convert the visible invalid draft rather than a stale committed "
            "percentage");
}

void invalidDirectionDoesNotEditThePreviousAxis() {
    ImageSplitDocument document;
    document.setImage(image({1200, 1}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    auto* direction = child<AdSegmented>(editor, "imageSplitDirection");
    direction->setCurrentValue(QStringLiteral("horizontal"));
    require(!editor.configurationValid() &&
                direction->currentValue() == QStringLiteral("horizontal") &&
                document.settings().direction == ImageSplitDirection::Vertical,
            "an invalid direction draft must remain displayed while preserving valid geometry");
    const auto previous = document.state();
    click(editor.canvas(), {editor.canvas()->imageToViewport({300, 0}).x(), 12});
    require(
        document.state() == previous,
        "an invalid displayed horizontal direction must not allow edits to the old vertical axis");
    child<AdInputNumber>(editor, "imageSplitCount")->setValue(1);
    require(editor.configurationValid() &&
                document.settings().direction == ImageSplitDirection::Horizontal,
            "correcting the count must commit the displayed direction and count together");
}

void submissionCommitsValidPendingDraftsOnly() {
    ImageSplitDocument document;
    document.setImage(image({1001, 800}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    auto* count = child<AdInputNumber>(editor, "imageSplitCount");
    auto* text = count->findChild<QLineEdit*>();
    require(text != nullptr, "number control must expose its text editor");
    text->setFocus();
    text->selectAll();
    QKeyEvent three(QEvent::KeyPress, Qt::Key_3, Qt::NoModifier, QStringLiteral("3"));
    QApplication::sendEvent(text, &three);
    require(editor.configurationValid() && count->displayText() == QStringLiteral("3") &&
                document.settings().count == 2 && !document.canUndo(),
            "valid keyboard drafts must wait for a commit without changing document history");
    require(editor.commitPendingSettings() && document.settings().count == 3 &&
                document.layout().outputCount() == 3,
            "submission must commit the visible valid draft before using output geometry");
    document.undo();
    require(document.settings().count == 2 && !document.canUndo() && document.canRedo(),
            "submitting one pending draft must create exactly one undo operation");
    document.redo();
    const auto beforeInvalid = document.state();
    text->setFocus();
    text->selectAll();
    QKeyEvent zero(QEvent::KeyPress, Qt::Key_0, Qt::NoModifier, QStringLiteral("0"));
    QApplication::sendEvent(text, &zero);
    require(!editor.commitPendingSettings() && !editor.configurationValid() &&
                document.state() == beforeInvalid,
            "submission must reject invalid pending drafts without changing output geometry");
}

void zoomedCanvasUsesFullResolutionPixels() {
    ImageSplitDocument document;
    QImage source({4096, 512}, QImage::Format_ARGB32_Premultiplied);
    source.fill(Qt::red);
    source.setPixelColor(2048, 256, Qt::blue);
    QImage preview({2048, 256}, QImage::Format_ARGB32_Premultiplied);
    preview.fill(Qt::green);
    document.setImage(source, preview);
    ImageSplitPreviewCanvas canvas(&document);
    canvas.resize(500, 400);
    canvas.show();
    pump();
    const auto pixelCenter = canvas.imageToViewport({2048.5, 256.5});
    wheel(&canvas, pixelCenter, 3600);
    require(canvas.zoom() > 2, "fixture must magnify individual source pixels");
    QImage output(canvas.size(), QImage::Format_ARGB32_Premultiplied);
    output.fill(Qt::transparent);
    QPainter painter(&output);
    canvas.render(&painter);
    painter.end();
    const auto position = canvas.imageToViewport({2048.5, 256.5});
    const QColor color = output.pixelColor(static_cast<int>(std::floor(position.x())),
                                           static_cast<int>(std::floor(position.y())));
    require(
        color == QColor(Qt::blue) && document.image() == source,
        "zoomed previews must draw full-resolution source pixels while preserving the original");
}

void denseGuidesDoNotHideLaterReadableRegions() {
    ImageSplitDocument document;
    QImage source(QSize(5001, 300), QImage::Format_ARGB32_Premultiplied);
    source.fill(Qt::red);
    document.setImage(source);
    auto settings = document.settings();
    settings.count = 5001;
    require(document.setSettings(settings), "dense fixture must generate one-pixel columns");
    while (document.layout().verticalLines.size() > 4001)
        require(document.removeLine(true, 4001), "dense fixture must merge its trailing columns");
    ImageSplitPreviewCanvas canvas(&document);
    canvas.resize(900, 400);
    canvas.show();
    pump();
    const auto state = document.state();
    QImage output(canvas.size(), QImage::Format_ARGB32_Premultiplied);
    output.fill(Qt::transparent);
    QPainter painter(&output);
    canvas.render(&painter);
    painter.end();
    const auto readableRegion = document.layout().regionAt(4001);
    const auto badge = canvas.imageToViewport(readableRegion.topLeft()) + QPointF(9, 9);
    require(output.pixelColor(static_cast<int>(std::floor(badge.x())),
                              static_cast<int>(std::floor(badge.y()))) != QColor(Qt::red),
            "a readable ordinal after four thousand thin columns must remain visible");
    require(document.state() == state && document.image() == source,
            "dense preview culling must preserve split geometry and source pixels");
}

void elongatedImagesKeepViewportAndLineInteractionsUsable() {
    constexpr int count = 1000000;
    ImageSplitDocument document;
    QImage source(QSize(count, 1), QImage::Format_ARGB32_Premultiplied);
    source.fill(Qt::red);
    QImage preview(QSize(2048, 1), QImage::Format_ARGB32_Premultiplied);
    preview.fill(Qt::red);
    document.setImage(source, preview);
    auto settings = document.settings();
    settings.count = count;
    require(document.setSettings(settings), "elongated fixture must allow one-pixel sections");
    ImageSplitEditorWidget editor(&document);
    show(editor);
    auto* canvas = editor.canvas();
    const auto original = document.state();
    const auto paint = [&] {
        QImage output(canvas->size(), QImage::Format_ARGB32_Premultiplied);
        output.fill(Qt::transparent);
        QPainter painter(&output);
        canvas->render(&painter);
    };
    paint();
    const auto anchor = canvas->imageToViewport({count / 2.0, .5});
    const auto anchoredPixel = canvas->viewportToImage(anchor);
    wheel(canvas, anchor, 240);
    require((canvas->viewportToImage(anchor) - anchoredPixel).manhattanLength() < .001,
            "extreme aspect ratios must preserve image-pixel zoom anchoring");
    mouse(canvas, QEvent::MouseButtonPress, anchor, Qt::MiddleButton, Qt::MiddleButton);
    mouse(canvas, QEvent::MouseMove, anchor + QPointF(10, 0), Qt::NoButton, Qt::MiddleButton);
    mouse(canvas, QEvent::MouseButtonRelease, anchor + QPointF(10, 0), Qt::MiddleButton,
          Qt::NoButton);
    paint();
    require(document.state() == original,
            "painting, zooming and panning a million-section image must preserve split geometry");
    click(canvas, canvas->imageToViewport({count / 2.0, .5}));
    QKeyEvent deletion(QEvent::KeyPress, Qt::Key_Delete, Qt::NoModifier);
    QApplication::sendEvent(canvas, &deletion);
    require(document.layout().outputCount() == count - 1,
            "hit testing must allow selecting and deleting a guide in a dense elongated image");
    document.undo();
    require(document.state() == original && document.layout().outputCount() == count &&
                document.image() == source,
            "dense line deletion must be undoable without altering full-resolution pixels");
}

class SplitTranslator final : public QTranslator {
  public:
    QString translate(const char* context, const char* source, const char*, int) const override {
        if (QByteArray(context) == "ImageSplitEditorWidget" && QByteArray(source) == "Reload")
            return QStringLiteral("重新加载");
        if (QByteArray(context) == "ImageSplitPreviewCanvas" &&
            QByteArray(source) == "Image split preview")
            return QStringLiteral("拆分预览");
        return {};
    }
};
void themesHighDpiAndLanguage() {
    auto& themes = adqt::theme::ThemeManager::instance();
    const auto original = themes.config();
    ImageSplitDocument document;
    document.setImage(image({1001, 800}));
    ImageSplitEditorWidget editor(&document);
    show(editor);
    const auto state = document.state();
    for (const auto theme : {adqt::theme::ThemeScheme::Light, adqt::theme::ThemeScheme::Dark}) {
        themes.setPreset(theme);
        for (const qreal scale : {1.0, 1.5, 2.0}) {
            QImage output(QSize(qRound(editor.canvas()->width() * scale),
                                qRound(editor.canvas()->height() * scale)),
                          QImage::Format_ARGB32_Premultiplied);
            output.setDevicePixelRatio(scale);
            output.fill(Qt::transparent);
            QPainter painter(&output);
            editor.canvas()->render(&painter);
            require(!output.isNull() && output.pixelColor(5, 5).alpha() == 255,
                    "rulers must render at fractional and integer high-DPI scales");
        }
    }
    themes.setConfig(original);
    SplitTranslator translator;
    QApplication::installTranslator(&translator);
    QEvent language(QEvent::LanguageChange);
    QApplication::sendEvent(&editor, &language);
    require(child<AdButton>(editor, "imageSplitReload")->text() == QStringLiteral("重新加载") &&
                editor.canvas()->accessibleName() == QStringLiteral("拆分预览") &&
                document.state() == state && !document.canUndo(),
            "runtime language changes must refresh controls without changing settings or history");
    QApplication::removeTranslator(&translator);
    QApplication::sendEvent(&editor, &language);
}
} // namespace

int main(int argc, char** argv) {
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    try {
        defaultsConditionalControlsAndActions();
        formValidationUnitsAndUndoableRegeneration();
        unitConversionDoesNotClampInvalidResults();
        percentageDraftsKeepExactEnteredPrecision();
        transformedRulersAndManualGestures();
        replacementResetBusyAndDrops();
        invalidDraftsSurviveFocusAndCanceledLoading();
        invalidDirectionDoesNotEditThePreviousAxis();
        submissionCommitsValidPendingDraftsOnly();
        zoomedCanvasUsesFullResolutionPixels();
        denseGuidesDoNotHideLaterReadableRegions();
        elongatedImagesKeepViewportAndLineInteractionsUsable();
        themesHighDpiAndLanguage();
        std::cout << "Image split editor tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
