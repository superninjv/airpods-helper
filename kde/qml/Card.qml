import QtQuick
import org.kde.plasma.components as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Controls.Pane {
    id: card
    property string heading: ""
    default property alias contents: body.data
    Layout.fillWidth: true
    Layout.minimumWidth: 0
    padding: 10
    background: Rectangle {
        radius: 9
        color: Qt.alpha(Kirigami.Theme.textColor, 0.045)
    }
    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        Kirigami.Heading {
            visible: card.heading !== ""
            text: card.heading
            level: 4
            Layout.fillWidth: true
        }
        ColumnLayout { id: body; spacing: Kirigami.Units.smallSpacing; Layout.fillWidth: true; Layout.minimumWidth: 0 }
    }
}
