import QtQuick
import QtQuick.Shapes

// The STAR/FLUX mark: a terminal prompt, `>_`, from the app icon. Drawn as
// whole pixels like a bitmap terminal font, 12 by 9 at the bar's 12px size,
// so it stays sharp; other sizes scale by a whole number for the same reason.
// The item is exactly the drawing, so centering the item centers the mark.
Shape {
  id: mark
  property color color: "white"
  property real size: 12
  readonly property int unit: Math.max(1, Math.round(size / 12))
  width: unit * 12
  height: unit * 9
  preferredRendererType: Shape.GeometryRenderer

  ShapePath {
    scale: Qt.size(mark.unit, mark.unit)
    strokeColor: "transparent"
    strokeWidth: -1
    fillColor: mark.color
    PathSvg {
      // The chevron, two pixels thick, one row at a time; then the cursor.
      path: "M0 0h2v1H0Z M1 1h2v1H1Z M2 2h2v1H2Z M3 3h2v1H3Z M4 4h2v1H4Z"
        + " M3 5h2v1H3Z M2 6h2v1H2Z M1 7h2v1H1Z M0 8h2v1H0Z"
        + " M7 8h5v1H7Z"
    }
  }

  // A half-strength row over the underscore, so it reads about a pixel and a
  // half thick: one pixel looked thin beside the chevron and two looked heavy.
  ShapePath {
    scale: Qt.size(mark.unit, mark.unit)
    strokeColor: "transparent"
    strokeWidth: -1
    fillColor: Qt.rgba(mark.color.r, mark.color.g, mark.color.b, mark.color.a * 0.5)
    PathSvg { path: "M7 7h5v1H7Z" }
  }
}
