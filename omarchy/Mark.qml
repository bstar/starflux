import QtQuick
import QtQuick.Shapes

// The STAR/FLUX mark: a terminal prompt in a square frame, the app icon cut
// down for bar sizes. Drawn as whole pixels on a 12-unit grid, like a bitmap
// terminal font, so it stays sharp at 12px; other sizes round to a whole
// multiple of 12 for the same reason.
Shape {
  id: mark
  property color color: "white"
  property real size: 12
  readonly property int unit: Math.max(1, Math.round(size / 12))
  width: unit * 12
  height: unit * 12
  preferredRendererType: Shape.GeometryRenderer

  ShapePath {
    scale: Qt.size(mark.unit, mark.unit)
    strokeColor: "transparent"
    strokeWidth: -1
    fillColor: mark.color
    fillRule: ShapePath.OddEvenFill
    PathSvg {
      // Frame, the prompt's chevron one step at a time, then its cursor.
      path: "M0 0H12V12H0Z M1 1V11H11V1Z"
        + " M3 3h1v1H3Z M4 4h1v1H4Z M5 5h1v2H5Z M4 7h1v1H4Z M3 8h1v1H3Z"
        + " M7 8h2v1H7Z"
    }
  }
}
