import QtQuick
import org.kde.plasma.components as Controls
import QtQuick.Layouts

Controls.SwitchDelegate {
    property string propertyKey
    property string method: ""
    Layout.fillWidth: true
    enabled: backend.available
    checked: !!backend.state[propertyKey]
    onClicked: {
        if (method) backend.control(method, checked)
        else backend.setting(propertyKey, checked)
        checked = Qt.binding(() => !!backend.state[propertyKey])
    }
}
