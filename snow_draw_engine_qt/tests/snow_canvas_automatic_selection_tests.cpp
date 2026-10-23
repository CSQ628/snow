#include "../../test-support/canvas_quick_selection_test_support.h"
#include "snow_draw_engine_qt/snow_canvas_runtime.h"

#include <QKeyEvent>
#include <QFocusEvent>

using canvas_quick_selection_test::mouse;
using canvas_quick_selection_test::require;

namespace {
void drawShape(SnowCanvasWidget& canvas, QPointF start = {60, 60}) {
    require(canvas.resetEditingState() && canvas.setCanvasTool(SnowCanvasTool::Shape),
            "prepare shape creation");
    mouse(canvas, QEvent::MouseButtonPress, start, Qt::LeftButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseMove, start + QPointF(100, 60), Qt::NoButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseButtonRelease, start + QPointF(100, 60), Qt::LeftButton,
          Qt::NoButton);
}

void selectionAndRuntimeReplacement() {
    SnowCanvasRuntime runtime;
    SnowCanvasWidget canvas(runtime);
    SnowCanvasWidget second(runtime);
    canvas.resize(400, 300);
    second.resize(400, 300);
    canvas.show();
    second.show();
    QApplication::processEvents();
    drawShape(canvas);
    require(runtime.selectedElementIds().isEmpty(), "automatic selection defaults off");
    require(runtime.setAutomaticallySelectNewlyCreatedElements(true) &&
                runtime.setQuickSelectionDisabledTools({SnowCanvasTool::Shape}),
            "enable automatic selection independently of quick selection");
    require(runtime.selectedElementIds().isEmpty(), "enabling leaves existing elements alone");
    drawShape(canvas, {220, 180});
    require(runtime.selectedElementIds().size() == 1 &&
                canvas.canvasStyleToolbarState().selectedElementCount == 1 &&
                second.canvasStyleToolbarState().selectedElementCount == 1 &&
                canvas.canvasTool() == SnowCanvasTool::Shape,
            "creation selects the new shape in every viewport and preserves the tool");
    const auto selected = runtime.selectedElementIds();
    require(runtime.setAutomaticallySelectNewlyCreatedElements(false) &&
                runtime.selectedElementIds() == selected,
            "disabling preserves the current selection");
    require(runtime.setAutomaticallySelectNewlyCreatedElements(true), "restore preference");
    const QByteArray document = runtime.serializeDocumentSession();
    const QByteArray history = runtime.serializeDocumentHistory();
    for (int replacement = 0; replacement < 4; ++replacement) {
        if (replacement == 0)
            require(runtime.reset(), "reset runtime");
        else if (replacement == 1)
            require(runtime.restoreDocumentSession(document), "restore document");
        else if (replacement == 2)
            require(runtime.restoreDocumentHistory(history), "restore history");
        else {
            SnowCanvasRuntime source;
            require(runtime.cloneDocumentSessionFrom(source), "clone another document");
        }
        drawShape(canvas);
        require(runtime.selectedElementIds().size() == 1 &&
                    second.canvasStyleToolbarState().selectedElementCount == 1,
                "runtime replacement preserves the preference and shared selection");
    }
}

void automaticSelectionExclusions() {
    SnowCanvasRuntimeConfig config;
    config.automaticallySelectNewlyCreatedElements = true;
    SnowCanvasRuntime runtime(config);
    SnowCanvasWidget canvas(runtime);
    canvas.resize(400, 300);
    canvas.show();
    QApplication::processEvents();
    const auto draw = [&](SnowCanvasTool tool) {
        require(canvas.resetEditingState() && canvas.setCanvasTool(tool), "activate excluded tool");
        mouse(canvas, QEvent::MouseButtonPress, {60, 60}, Qt::LeftButton, Qt::LeftButton);
        mouse(canvas, QEvent::MouseMove, {160, 120}, Qt::NoButton, Qt::LeftButton);
        mouse(canvas, QEvent::MouseButtonRelease, {160, 120}, Qt::LeftButton, Qt::NoButton);
    };
    for (const auto tool : {SnowCanvasTool::FreeDraw, SnowCanvasTool::PenFilter}) {
        require(runtime.setAutomaticSelectionExcludedTools(config.automaticSelectionExcludedTools),
                "apply default exclusions");
        draw(tool);
        require(runtime.selectedElementIds().isEmpty(),
                "default exclusions leave Pen and Pen filter unselected");
        require(canvas.undo() && runtime.setAutomaticSelectionExcludedTools({}),
                "clear exclusions");
        draw(tool);
        require(runtime.selectedElementIds().size() == 1,
                "removing an exclusion enables automatic selection");
        require(canvas.undo(), "remove test stroke");
    }
    require(runtime.setAutomaticSelectionExcludedTools({SnowCanvasTool::Shape}), "exclude shape");
    const QByteArray document = runtime.serializeDocumentSession();
    const QByteArray history = runtime.serializeDocumentHistory();
    for (int replacement = 0; replacement < 4; ++replacement) {
        if (replacement == 0)
            require(runtime.reset(), "reset runtime with exclusions");
        else if (replacement == 1)
            require(runtime.restoreDocumentSession(document), "restore document with exclusions");
        else if (replacement == 2)
            require(runtime.restoreDocumentHistory(history), "restore history with exclusions");
        else {
            SnowCanvasRuntime source;
            require(runtime.cloneDocumentSessionFrom(source), "clone document with exclusions");
        }
        draw(SnowCanvasTool::Shape);
        require(runtime.selectedElementIds().isEmpty(),
                "custom exclusions survive runtime replacement");
        require(canvas.undo(), "remove excluded shape");
        draw(SnowCanvasTool::FreeDraw);
        require(runtime.selectedElementIds().size() == 1,
                "removed defaults survive runtime replacement");
    }
}

void selectedShapeHasWorkingHandles() {
    SnowCanvasRuntimeConfig config;
    config.automaticallySelectNewlyCreatedElements = true;
    SnowCanvasRuntime runtime(config);
    SnowCanvasWidget canvas(runtime);
    canvas.resize(400, 300);
    canvas.show();
    QApplication::processEvents();
    drawShape(canvas);
    const QByteArray original = runtime.documentCheckpoint();
    mouse(canvas, QEvent::MouseButtonPress, {160, 120}, Qt::LeftButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseMove, {190, 150}, Qt::NoButton, Qt::LeftButton);
    mouse(canvas, QEvent::MouseButtonRelease, {190, 150}, Qt::LeftButton, Qt::NoButton);
    require(runtime.selectedElementIds().size() == 1 && runtime.documentCheckpoint() != original,
            "newly selected shape can be resized immediately");
    require(canvas.undo() && runtime.documentCheckpoint() == original,
            "resizing is separate from the single creation history entry");
    require(canvas.undo() && !canvas.canvasHistoryState().canUndo &&
                runtime.selectedElementIds().isEmpty(),
            "automatic selection adds no history entry or stale selection");
}

void textCommitRespectsDefaultExclusion() {
    for (const bool excluded : {true, false}) {
        SnowCanvasRuntimeConfig config;
        config.automaticallySelectNewlyCreatedElements = true;
        if (!excluded)
            config.automaticSelectionExcludedTools.remove(SnowCanvasTool::Text);
        SnowCanvasRuntime runtime(config);
        SnowCanvasWidget canvas(runtime);
        canvas.resize(400, 300);
        canvas.show();
        QApplication::processEvents();
        require(canvas.setCanvasTool(SnowCanvasTool::Text), "activate text tool");
        mouse(canvas, QEvent::MouseButtonPress, {200, 150}, Qt::LeftButton, Qt::LeftButton);
        mouse(canvas, QEvent::MouseButtonRelease, {200, 150}, Qt::LeftButton, Qt::NoButton);
        QKeyEvent type(QEvent::KeyPress, Qt::Key_A, Qt::NoModifier, QStringLiteral("new text"));
        QApplication::sendEvent(&canvas, &type);
        mouse(canvas, QEvent::MouseButtonPress, {25, 40}, Qt::RightButton, Qt::RightButton);
        mouse(canvas, QEvent::MouseButtonRelease, {25, 40}, Qt::RightButton, Qt::NoButton);
        require(
            !canvas.hasActiveTextEditing() &&
                runtime.selectedElementIds().size() == (excluded ? 0 : 1) &&
                (excluded || canvas.canvasStyleToolbarState().source ==
                                 SnowCanvasStyleToolbarSource::SelectedText) &&
                canvas.canvasTool() == SnowCanvasTool::Text,
            "new text respects its default exclusion and can be opted into automatic selection");
    }
}

void serialNumberLabelSelectionPolicy() {
    enum class Finish { Commit, RightClickCommit, Cancel, SwitchTool, FocusLoss };
    for (const bool enabled : {false, true}) {
        for (const bool excluded : {false, true}) {
            for (const auto finish : {Finish::Commit, Finish::RightClickCommit, Finish::Cancel,
                                      Finish::SwitchTool, Finish::FocusLoss}) {
                SnowCanvasRuntimeConfig config;
                config.automaticallySelectNewlyCreatedElements = enabled;
                if (excluded)
                    config.automaticSelectionExcludedTools.insert(SnowCanvasTool::SerialNumber);
                SnowCanvasRuntime runtime(config);
                SnowCanvasWidget canvas(runtime);
                SnowCanvasWidget second(runtime);
                canvas.resize(400, 300);
                second.resize(400, 300);
                canvas.show();
                QApplication::processEvents();
                require(canvas.setCanvasTool(SnowCanvasTool::SerialNumber), "activate badge tool");
                mouse(canvas, QEvent::MouseButtonPress, {60, 60}, Qt::LeftButton, Qt::LeftButton);
                mouse(canvas, QEvent::MouseMove, {160, 120}, Qt::NoButton, Qt::LeftButton);
                mouse(canvas, QEvent::MouseButtonRelease, {160, 120}, Qt::LeftButton, Qt::NoButton);
                require(canvas.hasActiveTextEditing(), "badge drag starts label editing");
                const auto selected = runtime.selectedElementIds();
                require(selected.size() == (enabled && !excluded ? 1 : 0),
                        "badge creation respects automatic selection");
                QKeyEvent type(QEvent::KeyPress, Qt::Key_A, Qt::NoModifier,
                               QStringLiteral("label"));
                QApplication::sendEvent(&canvas, &type);
                require(runtime.setAutomaticallySelectNewlyCreatedElements(!enabled),
                        "change the policy while editing without changing live selection");
                if (finish == Finish::SwitchTool) {
                    require(canvas.setCanvasTool(SnowCanvasTool::Select),
                            "commit by changing tool");
                } else if (finish == Finish::FocusLoss) {
                    QFocusEvent focusOut(QEvent::FocusOut, Qt::OtherFocusReason);
                    QApplication::sendEvent(&canvas, &focusOut);
                } else if (finish == Finish::RightClickCommit) {
                    mouse(canvas, QEvent::MouseButtonPress, {25, 240}, Qt::RightButton,
                          Qt::RightButton);
                    mouse(canvas, QEvent::MouseButtonRelease, {25, 240}, Qt::RightButton,
                          Qt::NoButton);
                } else {
                    QKeyEvent finishEdit(QEvent::KeyPress,
                                         finish == Finish::Cancel ? Qt::Key_Escape : Qt::Key_Return,
                                         finish == Finish::Cancel ? Qt::NoModifier
                                                                  : Qt::ControlModifier);
                    QApplication::sendEvent(&canvas, &finishEdit);
                }
                // A blank right click explicitly resets the serial tool and clears selection.
                const auto expectedSelection =
                    finish == Finish::RightClickCommit ? QJsonArray{} : selected;
                const bool selectionPreserved =
                    !canvas.hasActiveTextEditing() &&
                    runtime.selectedElementIds() == expectedSelection &&
                    second.canvasStyleToolbarState().selectedElementCount ==
                        expectedSelection.size();
                if (!selectionPreserved) {
                    std::cerr << "serial label completion: enabled=" << enabled
                              << ", excluded=" << excluded
                              << ", finish=" << static_cast<int>(finish)
                              << ", before=" << selected.size()
                              << ", after=" << runtime.selectedElementIds().size() << '\n';
                }
                require(selectionPreserved,
                        "label commits preserve selection and respect explicit tool resets");
                if (!expectedSelection.isEmpty())
                    require(canvas.canvasStyleToolbarState().source ==
                                SnowCanvasStyleToolbarSource::SelectedSerialNumber,
                            "label editing does not replace the selected badge with its label");
                require(canvas.setCanvasTool(SnowCanvasTool::Text),
                        "activate explicit label editing");
                mouse(canvas, QEvent::MouseButtonPress, {160, 120}, Qt::LeftButton, Qt::LeftButton);
                mouse(canvas, QEvent::MouseButtonRelease, {160, 120}, Qt::LeftButton, Qt::NoButton);
                if (!canvas.hasActiveTextEditing() || runtime.selectedElementIds().size() != 1) {
                    std::cerr << "serial label case: enabled=" << enabled
                              << ", excluded=" << excluded
                              << ", finish=" << static_cast<int>(finish) << '\n';
                }
                require(canvas.hasActiveTextEditing() && runtime.selectedElementIds().size() == 1,
                        "serial labels can still be explicitly selected for editing");
                const auto explicitlySelected = runtime.selectedElementIds();
                require(selected.isEmpty() || explicitlySelected != selected,
                        "explicit editing selects the label instead of its badge");
                QKeyEvent commit(QEvent::KeyPress, Qt::Key_Return, Qt::ControlModifier);
                QApplication::sendEvent(&canvas, &commit);
                require(!canvas.hasActiveTextEditing() &&
                            runtime.selectedElementIds() == explicitlySelected,
                        "manual label editing preserves its explicit selection");
            }
        }
    }
}

void speechBubbleSelectionPolicy() {
    enum class Finish { EmptyCommit, Commit, RightClickCommit, Cancel, SwitchTool, FocusLoss };
    for (const bool enabled : {false, true}) {
        for (const bool excluded : {false, true}) {
            for (const auto finish : {Finish::EmptyCommit, Finish::Commit, Finish::RightClickCommit,
                                      Finish::Cancel, Finish::SwitchTool, Finish::FocusLoss}) {
                SnowCanvasRuntimeConfig config;
                config.automaticallySelectNewlyCreatedElements = enabled;
                if (!excluded) {
                    config.automaticSelectionExcludedTools.remove(SnowCanvasTool::SpeechBubble);
                }
                config.quickSelectionDisabledTools.insert(SnowCanvasTool::SpeechBubble);
                SnowCanvasRuntime runtime(config);
                SnowCanvasWidget canvas(runtime);
                SnowCanvasWidget second(runtime);
                canvas.resize(400, 300);
                second.resize(400, 300);
                canvas.show();
                QApplication::processEvents();
                require(canvas.setCanvasTool(SnowCanvasTool::SpeechBubble), "activate bubble tool");
                mouse(canvas, QEvent::MouseButtonPress, {60, 60}, Qt::LeftButton, Qt::LeftButton);
                mouse(canvas, QEvent::MouseMove, {160, 120}, Qt::NoButton, Qt::LeftButton);
                mouse(canvas, QEvent::MouseButtonRelease, {160, 120}, Qt::LeftButton, Qt::NoButton);
                require(canvas.hasActiveTextEditing(),
                        "unselected bubbles still start text editing");
                const auto expectedCount = enabled && !excluded ? 1u : 0u;
                const auto verifySelection = [&] {
                    require(runtime.selectedElementIds().size() == expectedCount &&
                                canvas.canvasStyleToolbarState().selectedElementCount ==
                                    expectedCount &&
                                second.canvasStyleToolbarState().selectedElementCount ==
                                    expectedCount,
                            "bubble creation and text editing respect automatic selection in every "
                            "viewport");
                };
                verifySelection();
                if (finish != Finish::EmptyCommit) {
                    QKeyEvent type(QEvent::KeyPress, Qt::Key_A, Qt::NoModifier,
                                   QStringLiteral("bubble text"));
                    QApplication::sendEvent(&canvas, &type);
                }
                if (finish == Finish::SwitchTool) {
                    require(canvas.setCanvasTool(SnowCanvasTool::Select),
                            "commit by changing tool");
                } else if (finish == Finish::FocusLoss) {
                    QFocusEvent focusOut(QEvent::FocusOut, Qt::OtherFocusReason);
                    QApplication::sendEvent(&canvas, &focusOut);
                } else if (finish == Finish::RightClickCommit) {
                    mouse(canvas, QEvent::MouseButtonPress, {25, 240}, Qt::RightButton,
                          Qt::RightButton);
                    mouse(canvas, QEvent::MouseButtonRelease, {25, 240}, Qt::RightButton,
                          Qt::NoButton);
                } else {
                    QKeyEvent finishEdit(QEvent::KeyPress,
                                         finish == Finish::Cancel ? Qt::Key_Escape : Qt::Key_Return,
                                         finish == Finish::Cancel ? Qt::NoModifier
                                                                  : Qt::ControlModifier);
                    QApplication::sendEvent(&canvas, &finishEdit);
                }
                require(!canvas.hasActiveTextEditing(), "finish bubble text editing");
                verifySelection();
                require(canvas.canvasTool() == (finish == Finish::SwitchTool
                                                    ? SnowCanvasTool::Select
                                                    : SnowCanvasTool::SpeechBubble),
                        "bubble text editing retains the tool");
                if (!expectedCount) {
                    mouse(canvas, QEvent::MouseButtonDblClick, {110, 90}, Qt::LeftButton,
                          Qt::LeftButton);
                    require(canvas.hasActiveTextEditing() &&
                                runtime.selectedElementIds().size() == 1,
                            "excluded bubbles can still be selected and edited manually");
                    QKeyEvent commit(QEvent::KeyPress, Qt::Key_Return, Qt::ControlModifier);
                    QApplication::sendEvent(&canvas, &commit);
                    require(!canvas.hasActiveTextEditing() &&
                                runtime.selectedElementIds().size() == 1,
                            "manual editing preserves explicit selection regardless of the policy");
                }
            }
        }
    }
}
} // namespace

int main(int argc, char** argv) {
    QApplication app(argc, argv);
    selectionAndRuntimeReplacement();
    automaticSelectionExclusions();
    selectedShapeHasWorkingHandles();
    textCommitRespectsDefaultExclusion();
    serialNumberLabelSelectionPolicy();
    speechBubbleSelectionPolicy();
    return 0;
}
