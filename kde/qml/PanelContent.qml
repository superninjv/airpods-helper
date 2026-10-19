import QtQuick
import org.kde.plasma.components as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.extras as PlasmaExtras


Controls.Pane {
    id: root
    objectName: "panel-content"
    width: 420; height: 400
    padding: 0
    readonly property real minimumPanelWidth: Math.max(320, battery.implicitWidth + scroll.leftPadding + scroll.rightPadding)
    Layout.minimumWidth: minimumPanelWidth
    Layout.minimumHeight: header.implicitHeight + tabs.implicitHeight + 120
    signal closeRequested()
    readonly property var s: backend.state
    readonly property bool connected: backend.available && !!s.Connected
    readonly property string deviceName: (backend.devices.find(device => device.address.toUpperCase() === (s.Address || "").toUpperCase()) || {}).name || qsTr("AirPods")
    property bool pinned: false
    property bool plasmaHosted: false
    property bool previewErrors: false // Enable locally to preview a dismissible error.
    readonly property int preferredPanelHeight: header.implicitHeight + 16 + tabs.implicitHeight + 2
        + (errorMessage.visible ? errorMessage.implicitHeight + 24 : 0)
        + scroll.topPadding + scroll.bottomPadding + (footer.visible ? footer.implicitHeight : 0)
        + (root.connected ? battery.implicitHeight + (root.has("anc") ? listening.implicitHeight + 10 : 0)
            : equalizer.y + equalizer.implicitHeight)
    property string page: "controls"
    function navigate(destination) {
        page = destination
        scroll.contentItem.contentY = 0
    }
    function glyph(name, color) { return "image://airpods-glyph/" + name + "/" + color.toString().slice(1) }
    function has(feature) { return (s.Features || []).indexOf(feature) !== -1 }
    Shortcut { sequence: "Escape"; onActivated: root.closeRequested() }
    Connections {
        target: backend
        function onError(message) { errorMessage.text = message; errorMessage.visible = true }
    }
    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        focus: true
        Keys.onEscapePressed: root.closeRequested()
        RowLayout {
            id: header
            Layout.fillWidth: true
            Layout.margins: 8
            spacing: 8
            Image {
                objectName: "panel-icon"
                Layout.preferredWidth: 40; Layout.preferredHeight: 40
                source: root.glyph(root.has("headphones") ? "headphones" : "airpods", Kirigami.Theme.textColor)
                sourceSize: Qt.size(40 * Screen.devicePixelRatio, 40 * Screen.devicePixelRatio)
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 1
                Controls.Label {
                    objectName: "panel-title"
                    Layout.fillWidth: true
                    text: root.connected ? root.deviceName : qsTr("AirPods Helper")
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.1; font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }
                Controls.Label {
                    objectName: "panel-model"
                    Layout.fillWidth: true
                    text: root.connected ? (root.s.ModelName || root.s.Model || "") : ""
                    visible: text !== ""
                    font: Kirigami.Theme.smallFont
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
                    elide: Text.ElideRight
                }
                RowLayout {
                    spacing: 5
                    Rectangle {
                        objectName: "connection-dot"
                        implicitWidth: 6; implicitHeight: 6; radius: 3
                        color: root.connected ? backend.batteryColor(100) : Qt.alpha(Kirigami.Theme.textColor, 0.6)
                    }
                    Controls.Label {
                        text: root.connected ? qsTr("Connected") : backend.available ? qsTr("Disconnected") : qsTr("Daemon not running")
                        color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
                    }
                }
            }
            Controls.ToolButton {
                id: refreshButton
                implicitWidth: 36; implicitHeight: 36
                objectName: "refresh-panel"
                icon.name: "view-refresh"
                text: qsTr("Refresh")
                display: Controls.AbstractButton.IconOnly
                onClicked: backend.refresh()
                Controls.ToolTip {
                    objectName: "refresh-tooltip"
                    parent: refreshButton
                    y: refreshButton.height + Kirigami.Units.smallSpacing
                    text: refreshButton.text
                }
            }
            Controls.ToolButton {
                id: pinButton
                objectName: "pin-panel"
                implicitWidth: 36; implicitHeight: 36
                icon.name: root.pinned ? "window-unpin" : "window-pin"
                text: qsTr("Keep Open")
                display: Controls.AbstractButton.IconOnly
                checkable: true
                checked: root.pinned
                onClicked: root.pinned = checked
                Controls.ToolTip {
                    objectName: "pin-tooltip"
                    parent: pinButton
                    y: pinButton.height + Kirigami.Units.smallSpacing
                    text: pinButton.text
                }
            }
        }
        Controls.TabBar {
            id: tabs
            objectName: "panel-tabs"
            Layout.fillWidth: true
            currentIndex: root.page === "devices" ? 1 : root.page === "settings" ? 2 : 0
            background: Item {
                Rectangle {
                    objectName: "tab-divider"
                    anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                    height: 1
                    color: Kirigami.Theme.textColor
                    opacity: 0.2
                }
            }
            contentItem: ListView {
                implicitWidth: contentWidth
                implicitHeight: contentHeight
                model: tabs.contentModel
                currentIndex: tabs.currentIndex
                orientation: ListView.Horizontal
                interactive: false
                highlightMoveDuration: Kirigami.Units.longDuration
                highlightResizeDuration: 0
                highlight: Item {
                    Rectangle {
                        objectName: "tab-underline"
                        anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                        height: 2
                        color: Kirigami.Theme.highlightColor
                    }
                }
            }
            Controls.TabButton {
                objectName: "toggle-controls"
                text: qsTr("Controls")
                icon.name: "adjustlevels"
                display: Controls.AbstractButton.IconOnly
                onClicked: root.navigate(equalizer.editing ? "equalizer" : "controls")
                Controls.ToolTip.visible: hovered
                Controls.ToolTip.text: text
            }
            Controls.TabButton {
                objectName: "toggle-devices"
                text: qsTr("Devices")
                icon.name: "audio-headphones"
                display: Controls.AbstractButton.IconOnly
                onClicked: root.navigate("devices")
                Controls.ToolTip.visible: hovered
                Controls.ToolTip.text: text
            }
            Controls.TabButton {
                objectName: "toggle-settings"
                text: qsTr("Settings")
                icon.name: "configure"
                display: Controls.AbstractButton.IconOnly
                onClicked: root.navigate("settings")
                Controls.ToolTip.visible: hovered
                Controls.ToolTip.text: text
            }
        }
        Kirigami.InlineMessage {
            id: errorMessage
            objectName: "panel-error"
            Layout.fillWidth: true
            Layout.margins: 12
            type: Kirigami.MessageType.Error
            showCloseButton: true
            visible: root.previewErrors
            text: qsTr("Error preview: Could not connect to your AirPods. This is a test; nothing was changed.")
        }
        Controls.ScrollView {
            id: scroll
            objectName: "panel-scroll"
            Layout.fillWidth: true; Layout.fillHeight: true
            clip: true
            leftPadding: 12; rightPadding: 12 + effectiveScrollBarWidth; topPadding: 12; bottomPadding: 12
            contentWidth: availableWidth
            Controls.ScrollBar.horizontal.policy: Controls.ScrollBar.AlwaysOff
            ColumnLayout {
                width: scroll.availableWidth
                spacing: 10
                Card {
                    visible: !backend.available
                    heading: qsTr("Daemon unavailable")
                    Controls.Label {
                        Layout.fillWidth: true
                        text: qsTr("Install the daemon package, or start your existing airpods-daemon service.")
                        wrapMode: Text.WordWrap
                    }
                    Controls.Label {
                        Layout.fillWidth: true
                        text: backend.daemonError
                        wrapMode: Text.WordWrap
                        visible: text !== ""
                    }
                    Controls.Button { text: qsTr("Copy service command"); onClicked: backend.copyText("systemctl --user enable --now airpods-daemon.service") }
                    Controls.Button { text: qsTr("Copy diagnostic command"); onClicked: backend.copyText("airpods-cli doctor") }
                    Controls.Button { text: qsTr("Retry connection"); onClicked: backend.refresh() }
                }
                Kirigami.InlineMessage {
                    Layout.fillWidth: true
                    visible: backend.available && (root.s.EqStatus === undefined || root.s.AutoReconnect === undefined)
                    type: Kirigami.MessageType.Warning
                    text: qsTr("This daemon is older than the UI. Update it to enable all controls.")
                }
                Controls.Button {
                    id: connectButton
                    visible: root.page === "controls" && !root.connected && backend.available
                    text: qsTr("Connect your AirPods")
                    onClicked: root.navigate("devices")
                }
                Card {
                    id: battery
                    objectName: "battery-card"
                    visible: root.page === "controls" && root.connected
                    heading: qsTr("Battery")
                    FontMetrics {
                        id: batteryMetrics
                        font.family: Kirigami.Theme.defaultFont.family
                        font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.8
                        font.weight: Font.DemiBold
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Repeater {
                            model: root.has("headphones") ? ["Left"] : ["Left", "Right", "Case"]
                            delegate: Rectangle {
                                id: batteryCard
                                objectName: "battery-tile-" + modelData
                                required property string modelData
                                readonly property int charge: root.s["Battery" + modelData] ?? -1
                                Layout.fillWidth: true; Layout.preferredWidth: 1
                                Layout.minimumWidth: implicitWidth
                                implicitWidth: batteryBody.implicitWidth + 20
                                implicitHeight: batteryBody.implicitHeight + 20
                                radius: 9
                                color: Qt.alpha(Kirigami.Theme.textColor, 0.045)
                                ColumnLayout {
                                    id: batteryBody
                                    anchors.fill: parent; anchors.margins: 10
                                    spacing: 4
                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: 6
                                        Controls.Label {
                                            objectName: "battery-name-" + batteryCard.modelData
                                            Layout.fillWidth: true
                                            text: root.has("headphones") ? qsTr("Headphones") : batteryCard.modelData === "Left" ? qsTr("Left") : batteryCard.modelData === "Right" ? qsTr("Right") : qsTr("Case")
                                            color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
                                        }
                                        Controls.Label {
                                            objectName: "in-ear-" + batteryCard.modelData
                                            visible: !!root.s["Ear" + batteryCard.modelData]
                                            text: qsTr("In ear")
                                            font: Kirigami.Theme.smallFont
                                            color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
                                        }
                                    }
                                    RowLayout {
                                        spacing: 2
                                        Layout.minimumWidth: batteryMetrics.advanceWidth("100%") + 2 + batteryMetrics.height * 0.65
                                        Controls.Label {
                                            id: percentage
                                            objectName: "charge-" + batteryCard.modelData
                                            text: batteryCard.charge < 0 ? "—" : batteryCard.charge + "%"
                                            font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.8; font.weight: Font.DemiBold
                                            color: batteryCard.charge >= 0 && batteryCard.charge <= 20 ? backend.batteryColor(batteryCard.charge) : Kirigami.Theme.textColor
                                        }
                                        Image {
                                            objectName: "charging-" + batteryCard.modelData
                                            visible: !!root.s["Charging" + batteryCard.modelData]
                                            Layout.preferredHeight: percentage.implicitHeight * 0.65
                                            Layout.preferredWidth: Layout.preferredHeight
                                            fillMode: Image.PreserveAspectFit
                                            source: root.glyph("charging", backend.batteryColor(100))
                                            sourceSize: Qt.size(32 * Screen.devicePixelRatio, 32 * Screen.devicePixelRatio)
                                            Accessible.role: Accessible.StaticText
                                            Accessible.name: qsTr("Charging")
                                        }
                                    }
                                    Controls.ProgressBar {
                                        id: chargeBar
                                        objectName: "battery-bar-" + batteryCard.modelData
                                        Layout.fillWidth: true
                                        implicitHeight: 4
                                        implicitWidth: 0
                                        padding: 0
                                        from: 0; to: 100
                                        value: Math.max(0, batteryCard.charge)
                                        Accessible.name: batteryCard.modelData + qsTr(" battery")
                                        background: Rectangle {
                                            objectName: "battery-track-" + batteryCard.modelData
                                            implicitHeight: 4
                                            radius: height / 2
                                            border.width: 0
                                            color: Qt.alpha(Kirigami.Theme.textColor, 0.1)
                                        }
                                        contentItem: Item {
                                            Rectangle {
                                                objectName: "battery-fill-" + batteryCard.modelData
                                                anchors.verticalCenter: parent.verticalCenter
                                                width: chargeBar.position * parent.width
                                                height: 4
                                                radius: height / 2
                                                border.width: 0
                                                color: backend.batteryColor(batteryCard.charge)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Card {
                    id: listening
                    visible: root.page === "controls" && root.connected && root.has("anc")
                    objectName: "listening-card"
                    heading: qsTr("Listening mode")
                    GridLayout {
                        id: modeGrid
                        Layout.fillWidth: true
                        columns: width >= (root.has("adaptive") ? 4 : 3) * (modeMetrics.averageCharacterWidth * 7 + Kirigami.Units.largeSpacing * 2) ? (root.has("adaptive") ? 4 : 3) : 2
                        rowSpacing: 5; columnSpacing: 5
                        FontMetrics { id: modeMetrics; font: Kirigami.Theme.defaultFont }
                        FontMetrics { id: modeLabelMetrics; font: Kirigami.Theme.smallFont }
                        readonly property var modes: [
                                {text: qsTr("Off"), value: "off"},
                                {text: qsTr("Noise Cancel"), value: "noise"},
                                {text: qsTr("Transparency"), value: "transparency"}
                            ].concat(root.has("adaptive") ? [{text: qsTr("Adaptive"), value: "adaptive"}] : [])
                        readonly property real longestLabelWidth: modes.reduce((longest, mode) => Math.max(longest, modeLabelMetrics.advanceWidth(mode.text)), 1)
                        readonly property real labelPointSize: Math.max(1, modeLabelMetrics.font.pointSize * Math.min(1,
                            Math.max(1, (width - columnSpacing * (columns - 1)) / columns - 2 * Kirigami.Units.smallSpacing - 4) / longestLabelWidth))
                        Repeater {
                            model: modeGrid.modes
                            delegate: Controls.Button {
                                id: modeButton
                                required property var modelData
                                objectName: "anc-" + modelData.value
                                Layout.fillWidth: true; Layout.preferredWidth: 1
                                Layout.fillHeight: true
                                text: modelData.text
                                checkable: true
                                checked: root.s.AncMode === modelData.value
                                Accessible.role: Accessible.RadioButton
                                padding: Kirigami.Units.smallSpacing * 2
                                leftPadding: Kirigami.Units.smallSpacing; rightPadding: Kirigami.Units.smallSpacing
                                // Tall tiles expose seams in some themes' sliced button gradients.
                                background: Rectangle {
                                    objectName: "anc-background-" + modeButton.modelData.value
                                    radius: 6
                                    color: modeButton.checked || modeButton.down ? Kirigami.Theme.highlightColor : Kirigami.Theme.backgroundColor
                                    border.width: modeButton.visualFocus || modeButton.hovered ? 1 : 0
                                    border.color: modeButton.visualFocus ? Kirigami.Theme.focusColor : Kirigami.Theme.hoverColor
                                }
                                onClicked: {
                                    backend.control("SetAncMode", modelData.value)
                                    checked = Qt.binding(() => root.s.AncMode === modelData.value)
                                }
                                contentItem: ColumnLayout {
                                    spacing: 5
                                    Image {
                                        Layout.alignment: Qt.AlignHCenter
                                        objectName: "anc-icon-" + modeButton.modelData.value
                                        Layout.minimumWidth: Kirigami.Units.iconSizes.large; Layout.maximumWidth: Kirigami.Units.iconSizes.large
                                        Layout.minimumHeight: Kirigami.Units.iconSizes.large; Layout.maximumHeight: Kirigami.Units.iconSizes.large
                                        source: root.glyph(modeButton.modelData.value, modeButton.checked || modeButton.down ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor)
                                        sourceSize: Qt.size(Kirigami.Units.iconSizes.large * Screen.devicePixelRatio, Kirigami.Units.iconSizes.large * Screen.devicePixelRatio)
                                    }
                                    Controls.Label {
                                        Layout.fillWidth: true
                                        Layout.minimumHeight: modeMetrics.height; Layout.maximumHeight: modeMetrics.height
                                        objectName: "anc-label-" + modeButton.modelData.value
                                        text: modeButton.text
                                        font.family: modeLabelMetrics.font.family
                                        font.pointSize: modeGrid.labelPointSize
                                        maximumLineCount: 1
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        wrapMode: Text.NoWrap
                                        color: modeButton.checked || modeButton.down ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                                    }
                                }
                            }
                        }
                    }
                    ColumnLayout {
                        visible: root.has("adaptive") && root.s.AncMode === "adaptive"
                        Layout.fillWidth: true
                        RowLayout {
                            Controls.Label { text: qsTr("Adaptive level"); Layout.fillWidth: true }
                            Controls.Label { text: Math.round(adaptiveSlider.value); color: Qt.alpha(Kirigami.Theme.textColor, 0.6) }
                        }
                        Controls.Slider {
                            id: adaptiveSlider
                            Layout.fillWidth: true
                            from: 0; to: 100; stepSize: 1
                            value: root.s.AdaptiveNoiseLevel ?? 50
                            Accessible.name: qsTr("Adaptive noise level")
                            onPressedChanged: if (!pressed) backend.control("SetAdaptiveNoiseLevel", value)
                            onMoved: if (!pressed) backend.control("SetAdaptiveNoiseLevel", value)
                        }
                    }
                }
                Equalizer {
                    id: equalizer
                    objectName: "equalizer"
                    Layout.maximumWidth: scroll.availableWidth
                    visible: backend.available && (root.page === "controls" || root.page === "equalizer")
                    onEditRequested: root.navigate("equalizer")
                    onFinished: root.navigate("controls")
                }
                Card {
                    visible: root.page === "controls" && root.connected
                    heading: qsTr("Listening options")
                    StateSwitch {
                        visible: root.has("ca")
                        text: qsTr("Conversational awareness")
                        propertyKey: "ConversationalAwareness"; method: "SetConversationalAwareness"
                    }
                    StateSwitch {
                        visible: root.has("one_bud_anc")
                        text: qsTr("Noise cancellation with one earbud")
                        propertyKey: "OneBudAnc"; method: "SetOneBudAnc"
                    }
                    StateSwitch {
                        id: volumeSwipe
                        visible: !root.has("headphones") && (root.has("volume_swipe") || ["A2698", "A2699", "A2700", "A2931", "A2968", "A3047", "A3048", "A3049", "A3063", "A3064", "A3065", "A3122"].indexOf(root.s.Model) !== -1)
                        objectName: "volume-swipe"
                        text: qsTr("Volume swipe")
                        propertyKey: "VolumeSwipe"; method: "SetVolumeSwipe"
                    }
                    Controls.ComboBox {
                        flat: true
                        objectName: "mic-mode"
                        Layout.fillWidth: true
                        visible: !root.has("headphones")
                        textRole: "text"; valueRole: "value"
                        model: [{text: qsTr("Microphone: automatic"), value: "auto"}, {text: qsTr("Microphone: left"), value: "left"}, {text: qsTr("Microphone: right"), value: "right"}]
                        currentIndex: Math.max(0, ["auto", "left", "right"].indexOf(root.s.MicMode || "auto"))
                        onActivated: backend.control("SetMicMode", currentValue)
                        Accessible.name: qsTr("Microphone")
                    }
                    Controls.Control {
                        id: micStatus
                        objectName: "mic-status"
                        visible: !!root.s.MicStatus && root.s.MicStatus !== "off"
                        Layout.fillWidth: true
                        leftPadding: volumeSwipe.leftPadding; rightPadding: volumeSwipe.rightPadding
                        topPadding: volumeSwipe.topPadding; bottomPadding: volumeSwipe.bottomPadding
                        implicitHeight: Math.max(volumeSwipe.implicitHeight, contentItem.implicitHeight + topPadding + bottomPadding)
                        Accessible.role: Accessible.StaticText
                        Accessible.name: qsTr("AirPods microphone: %1").arg(micStatusValue.text)
                        Accessible.description: root.s.MicError || ""
                        contentItem: ColumnLayout {
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Kirigami.Units.largeSpacing
                                Controls.Label {
                                    objectName: "mic-status-label"
                                    Layout.fillWidth: true
                                    text: qsTr("AirPods microphone")
                                    wrapMode: Text.WordWrap
                                }
                                Controls.Label {
                                    id: micStatusValue
                                    objectName: "mic-status-value"
                                    text: ({idle: qsTr("Ready"), starting: qsTr("Starting"), streaming: qsTr("In use"), unavailable: qsTr("Unavailable"), error: qsTr("Error")})[root.s.MicStatus] || root.s.MicStatus || ""
                                    color: root.s.MicError ? Kirigami.Theme.negativeTextColor : Qt.alpha(Kirigami.Theme.textColor, 0.6)
                                }
                            }
                            Controls.Label {
                                objectName: "mic-status-error"
                                Layout.fillWidth: true
                                text: root.s.MicError || ""
                                visible: text !== ""
                                color: Kirigami.Theme.negativeTextColor
                                wrapMode: Text.WordWrap
                            }
                        }
                    }
                }
                DevicesPane { visible: root.page === "devices" }
                SettingsPane { visible: root.page === "settings"; plasmaHosted: root.plasmaHosted }

            }
        }

        PlasmaExtras.PlasmoidHeading {
            id: footer
            objectName: "panel-footer"
            visible: !root.plasmaHosted
            position: Controls.ToolBar.Footer
            Layout.fillWidth: true
            contentItem: Item { implicitHeight: Kirigami.Units.smallSpacing }
        }

    }
}
