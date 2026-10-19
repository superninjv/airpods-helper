import QtQuick
import org.kde.plasma.components as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.core as PlasmaCore

PlasmaCore.Dialog {
    id: root
    title: qsTr("AirPods Helper")
    type: PlasmaCore.Dialog.AppletPopup
    location: PlasmaCore.Types.BottomEdge
    floating: 1
    backgroundHints: PlasmaCore.Dialog.StandardBackground
    visible: false
    readonly property alias preferredPanelHeight: panel.preferredPanelHeight
    property alias pinned: panel.pinned
    property alias page: panel.page
    onClosing: function(close) { close.accepted = false; root.visible = false }
    mainItem: PanelContent {
        id: panel
        onCloseRequested: root.visible = false
    }
}
