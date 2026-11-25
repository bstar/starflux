import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// A read-only view of STAR/FLUX agents. Everything comes from the STAR/FLUX
// automation API; the widget keeps no state of its own and its only action
// is navigation (starflux-open), never input to an agent.
BarWidget {
  id: root
  moduleName: "bstar.starflux"

  // Release version, kept equal to Cargo.toml. Compatibility with the
  // installed STAR/FLUX is decided from its `capabilities` reply instead,
  // because the widget updates by git and the app by its package manager.
  readonly property string pluginVersion: "0.2.0"
  readonly property int supportedApi: 1
  readonly property var requiredMethods: ["projects.list", "agents.live", "frontends.list", "frontends.command"]
  readonly property string installGuide: "https://github.com/bstar/starflux/blob/main/docs/omarchy.md"
  readonly property string pluginDir: Qt.resolvedUrl(".").toString().replace(/^file:\/\//, "")
  readonly property var needsYouStates: ["needs_input", "waiting_for_approval", "failed", "backend_unavailable"]

  // -1 not yet checked; 0 `starflux` is not on PATH; 1 it is.
  property int installState: -1
  // "" until the first reply; then "ok", "unreachable" or "incompatible".
  property string apiState: ""
  property string apiDetail: ""
  property var agents: []
  property var projects: ({})
  property bool popupOpen: false
  readonly property bool opened: popupOpen
  property bool popoutSwitchClosing: false

  readonly property var waiting: agents.filter(function(a) { return root.needsYouStates.indexOf(a.state) !== -1 })
  readonly property var others: agents.filter(function(a) { return root.needsYouStates.indexOf(a.state) === -1 })
  readonly property int workingCount: agents.filter(function(a) { return a.state === "working" }).length
  readonly property color ink: bar ? bar.foreground : Color.foreground
  readonly property string face: bar ? bar.fontFamily : Style.font.family
  readonly property int refreshMs: Math.max(5, Number(setting("refreshIntervalSec", 15)) || 15) * 1000

  readonly property string unavailableText: installState === 0 ? "STAR/FLUX isn't installed"
    : apiState === "incompatible" ? "Update STAR/FLUX to use this widget"
    : apiState === "unreachable" ? "STAR/FLUX isn't responding"
    : ""

  function close() { popupOpen = false }
  function open() { popoutSwitchClosing = false; popupOpen = true }
  function closeForPopoutSwitch() {
    popoutSwitchClosing = true
    close()
  }

  function refresh() {
    if (installState === 1 && !pollProcess.running) pollProcess.running = true
    else if (installState !== 1 && !installProcess.running) installProcess.running = true
  }

  function label(agent) {
    var p = projects[agent.project]
    return (agent.project_label || (p && p.label) || agent.project) + " · " + agent.pane
  }
  function where(agent) {
    var p = projects[agent.project]
    return p && p.host ? String(p.host) : "local"
  }
  function stateText(state) {
    switch (state) {
    case "needs_input": return "needs input"
    case "waiting_for_approval": return "needs approval"
    case "backend_unavailable": return "backend down"
    default: return String(state || "unknown")
    }
  }

  function applyReplies(text) {
    var replies = {}
    String(text).split("\n").forEach(function(line) {
      if (line.trim() === "") return
      try {
        var reply = JSON.parse(line)
        replies[reply.id] = reply
      } catch (e) {}
    })
    var caps = replies.caps
    if (!caps || !caps.ok) {
      apiState = "unreachable"
      apiDetail = caps && caps.error ? String(caps.error.message || "") : ""
      agents = []
      return
    }
    var methods = (caps.result.methods || []).map(function(m) { return m.method })
    var missing = requiredMethods.filter(function(m) { return methods.indexOf(m) === -1 })
    if (Number(caps.result.version) !== supportedApi || missing.length > 0) {
      apiState = "incompatible"
      apiDetail = missing.length > 0 ? "missing " + missing.join(", ") : "API version " + caps.result.version
      agents = []
      return
    }
    var map = {}
    if (replies.projects && replies.projects.ok) {
      (replies.projects.result.projects || []).forEach(function(p) { map[p.id] = p })
    }
    projects = map
    agents = replies.agents && replies.agents.ok ? (replies.agents.result.agents || []) : []
    apiState = "ok"
    apiDetail = replies.agents && !replies.agents.ok && replies.agents.error ? String(replies.agents.error.message || "") : ""
  }

  onPopupOpenChanged: {
    if (popupOpen) {
      popoutSwitchClosing = false
      refresh()
    }
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  IpcHandler {
    target: "bstar.starflux"
    function open(): void { root.open() }
    function close(): void { root.close() }
    function toggle(): void { root.popupOpen = !root.popupOpen }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.unavailableText !== "" || root.apiState === "" ? "󱚣"
      : root.waiting.length > 0 ? "󱚣 " + root.waiting.length
      : root.vertical || root.workingCount === 0 ? "󱚣" : "󱚣 " + root.workingCount
    // The bar's urgent color, the same signal other widgets use for "look here".
    active: root.waiting.length > 0
    fontSize: Style.font.body
    horizontalMargin: 6
    tooltipText: root.unavailableText !== "" ? root.unavailableText
      : root.waiting.length > 0 ? root.waiting.length + (root.waiting.length === 1 ? " agent needs you" : " agents need you")
      : root.agents.length + " agents · " + root.workingCount + " working"
    onPressed: function(b) {
      if (b === Qt.LeftButton) root.popupOpen = !root.popupOpen
    }
  }

  // Absent is a state to explain, not an error: the plugin installs by git
  // and cannot install STAR/FLUX. Checked again on each refresh until found.
  Process {
    id: installProcess
    command: ["sh", "-c", "command -v starflux"]
    running: true
    onExited: function(exitCode) {
      root.installState = exitCode === 0 ? 1 : 0
      if (root.installState === 1) root.refresh()
    }
  }

  // One process, three requests, answered in order over JSON Lines.
  Process {
    id: pollProcess
    command: ["sh", "-c",
      "printf '%s\\n' "
      + "'{\"version\":1,\"id\":\"caps\",\"method\":\"capabilities\",\"params\":{}}' "
      + "'{\"version\":1,\"id\":\"projects\",\"method\":\"projects.list\",\"params\":{}}' "
      + "'{\"version\":1,\"id\":\"agents\",\"method\":\"agents.live\",\"params\":{\"limit\":16}}' "
      + "| timeout 10 starflux --api-stdio"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyReplies(text)
    }
    onExited: function(exitCode) {
      if (exitCode === 127) root.installState = 0
      else if (exitCode !== 0 && root.apiState !== "incompatible") root.apiState = "unreachable"
    }
  }

  Process {
    id: openProcess
  }

  function openAgent(agent) {
    if (openProcess.running) return
    openProcess.command = [pluginDir + "starflux-open", String(agent.project), String(agent.pane), String(agent.epoch || "")]
    openProcess.running = true
    close()
  }

  Timer {
    interval: root.popupOpen ? 3000 : root.refreshMs
    repeat: true
    running: true
    onTriggered: root.refresh()
  }

  KeyboardPanel {
    id: popup
    anchorItem: root
    bar: root.bar
    owner: root
    open: root.popupOpen
    focusTarget: keyCatcher
    padding: Style.space(10)
    contentWidth: popup.fittedContentWidth(Style.space(400))
    contentHeight: popup.fittedContentHeight(content.implicitHeight, Style.space(600))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) {
        if (root.bar && root.bar.switchPanelFrom) root.bar.switchPanelFrom(root, direction)
      }

      ScrollView {
        id: scrollArea
        anchors.fill: parent
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
        ScrollBar.vertical.policy: content.implicitHeight > height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff

        Column {
          id: content
          width: scrollArea.availableWidth
          spacing: Style.space(8)

          SectionHeader { label: "STAR/FLUX · AGENTS"; foreground: root.ink; fontFamily: root.face }

          Text {
            width: parent.width
            visible: root.unavailableText !== ""
            textFormat: Text.PlainText
            text: root.unavailableText + (root.apiDetail ? " (" + root.apiDetail + ")" : "")
            color: root.ink
            font.family: root.face
            font.pixelSize: Style.font.bodySmall
            wrapMode: Text.WordWrap
          }
          Text {
            width: parent.width
            visible: root.installState === 0
            textFormat: Text.StyledText
            text: "The widget needs STAR/FLUX. <a href=\"" + root.installGuide + "\">How to install it</a>"
            linkColor: root.ink
            color: Color.muted
            font.family: root.face
            font.pixelSize: Style.font.caption
            wrapMode: Text.WordWrap
            onLinkActivated: function(link) { Qt.openUrlExternally(link) }
          }
          Text {
            visible: root.apiState === "ok" && root.agents.length === 0
            text: "No agents running"
            color: Color.muted
            font.family: root.face
            font.pixelSize: Style.font.caption
          }

          SectionHeader {
            visible: root.waiting.length > 0
            label: "NEEDS YOU"
            foreground: root.ink
            fontFamily: root.face
          }
          Repeater {
            model: root.waiting
            delegate: agentRow
          }

          SectionHeader {
            visible: root.others.length > 0
            label: root.waiting.length > 0 ? "OTHER AGENTS" : "RUNNING"
            foreground: root.ink
            fontFamily: root.face
          }
          Repeater {
            model: root.others
            delegate: agentRow
          }

          Text {
            width: parent.width
            visible: root.apiState === "ok"
            text: "Click an agent to open its workspace. Nothing is sent to it."
            color: Color.muted
            font.family: root.face
            font.pixelSize: Style.font.caption
            wrapMode: Text.WordWrap
          }
        }
      }
    }
  }

  Component {
    id: agentRow
    Item {
      required property var modelData
      width: content.width
      height: Style.space(36)
      readonly property bool urgent: root.needsYouStates.indexOf(modelData.state) !== -1

      Rectangle {
        anchors.fill: parent
        radius: Style.space(4)
        color: rowMouse.containsMouse ? Qt.rgba(root.ink.r, root.ink.g, root.ink.b, 0.08) : "transparent"
      }
      Column {
        x: Style.space(6)
        width: parent.width - Style.space(120)
        anchors.verticalCenter: parent.verticalCenter
        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: root.label(modelData)
          color: root.ink
          font.family: root.face
          font.pixelSize: Style.font.bodySmall
          font.bold: true
          elide: Text.ElideRight
        }
        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: String(modelData.harness || "agent") + " · " + root.where(modelData)
          color: Color.muted
          font.family: root.face
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }
      Text {
        width: Style.space(108)
        anchors.right: parent.right
        anchors.rightMargin: Style.space(6)
        anchors.verticalCenter: parent.verticalCenter
        horizontalAlignment: Text.AlignRight
        textFormat: Text.PlainText
        text: root.stateText(modelData.state)
        color: parent.urgent || modelData.state === "working" ? Color.accent : Color.muted
        font.family: root.face
        font.pixelSize: Style.font.caption
        font.bold: parent.urgent
      }
      MouseArea {
        id: rowMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.openAgent(parent.modelData)
      }
    }
  }
}
