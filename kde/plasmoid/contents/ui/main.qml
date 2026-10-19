import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.plasmoid

PlasmoidItem {
    id: root
    Plasmoid.status: PlasmaCore.Types.ActiveStatus
    Plasmoid.icon: "audio-headphones"
    property bool ready: false
    property string toolTipMainText: Plasmoid.backend.state.ModelName || qsTr("AirPods Helper")
    property string toolTipSubText: {
        const state = Plasmoid.backend.state
        if (!state.Connected) return qsTr("Disconnected")
        const headphones = (state.Features || []).includes("headphones")
        return (headphones ? ["Left"] : ["Left", "Right", "Case"]).filter(part => state["Battery" + part] >= 0)
            .map(part => (headphones ? qsTr("Battery") : part) + ": " + state["Battery" + part] + "%"
                + (state["Charging" + part] ? " · " + qsTr("Charging") : "")).join("\n") || qsTr("Connected")
    }
    Component.onCompleted: {
        Plasmoid.prepareEngine(root)
        ready = true
    }
    compactRepresentation: Item {
        Layout.minimumWidth: Plasmoid.formFactor === PlasmaCore.Types.Horizontal ? height : Kirigami.Units.iconSizes.small
        Layout.minimumHeight: Plasmoid.formFactor === PlasmaCore.Types.Vertical ? width : Kirigami.Units.iconSizes.small
        activeFocusOnTab: true
        Accessible.role: Accessible.Button
        Accessible.name: root.toolTipMainText
        Accessible.onPressAction: root.expanded = !root.expanded
        Keys.onSpacePressed: root.expanded = !root.expanded
        Keys.onReturnPressed: root.expanded = !root.expanded
        Kirigami.Icon {
            anchors.fill: parent
            source: Plasmoid.batteryIcon
        }
        MouseArea {
            anchors.fill: parent
            property bool wasExpanded: false
            onPressed: wasExpanded = root.expanded
            onClicked: root.expanded = !wasExpanded
        }
    }
    fullRepresentation: Loader {
        id: representation
        property var backend: Plasmoid.backend
        active: root.ready
        source: "qrc:/qt/qml/AirPods/qml/PanelContent.qml"
        Layout.minimumWidth: item ? item.minimumPanelWidth : 320
        Layout.minimumHeight: 240
        Layout.preferredWidth: Math.max(420, Layout.minimumWidth)
        Layout.maximumWidth: Infinity
        Layout.maximumHeight: Infinity
        onLoaded: {
            item.plasmaHosted = true
            Qt.callLater(function() {
                // Settle both parent widths and wrapped child heights before measuring.
                function polish(item) {
                    item.ensurePolished()
                    for (const child of item.children) polish(child)
                    item.ensurePolished()
                }
                polish(item)
                // Only an initial hint; Plasma owns manual resizing and persistence.
                representation.Layout.preferredHeight = item.preferredPanelHeight
            })
        }
        Connections {
            target: representation.item
            function onCloseRequested() { root.expanded = false }
            function onPinnedChanged() { root.hideOnWindowDeactivate = !representation.item.pinned }
        }
    }
}
