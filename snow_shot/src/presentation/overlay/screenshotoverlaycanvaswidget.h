#ifndef SNOW_SHOT_PRESENTATION_SCREENSHOTOVERLAYCANVASWIDGET_H
#define SNOW_SHOT_PRESENTATION_SCREENSHOTOVERLAYCANVASWIDGET_H

#include "snow_draw_engine_qt/snow_canvas_widget.h"

class QPaintEvent;
class QFocusEvent;
class SnowCanvasRuntime;

class ScreenshotOverlayCanvasWidget final : public SnowCanvasWidget {
  public:
    explicit ScreenshotOverlayCanvasWidget(SnowCanvasRuntime& runtime, QWidget* parent = nullptr);

  protected:
    void focusInEvent(QFocusEvent* event) override;
    void paintEvent(QPaintEvent* event) override;
};

#endif // SNOW_SHOT_PRESENTATION_SCREENSHOTOVERLAYCANVASWIDGET_H
