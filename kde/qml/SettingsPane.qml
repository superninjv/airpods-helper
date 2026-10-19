import QtQuick
import org.kde.plasma.components as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: settings
    property bool plasmaHosted: false
    Layout.fillWidth: true
    Card {
        objectName: "settings-page"
        heading: qsTr("Settings")
        Repeater {
            model: [
                {text: qsTr("Pause when an earbud is removed"), key: "PauseOnRemoval"},
                {text: qsTr("Resume when reinserted"), key: "ResumeOnInsert"},
                {text: qsTr("Reconnect automatically"), key: "AutoReconnect"},
                {text: qsTr("Apply equalizer on connection"), key: "EqAutoLoad"},
                {text: qsTr("Enable AirPods microphone source"), key: "MicSource"}
            ]
            delegate: StateSwitch {
                required property var modelData
                objectName: "setting-" + modelData.key
                text: modelData.text; propertyKey: modelData.key
            }
        }
        Controls.SwitchDelegate {
            visible: !settings.plasmaHosted
            Layout.fillWidth: true
            objectName: "setting-login"
            text: qsTr("Start at login")
            checked: backend.startOnLogin
            onClicked: {
                backend.setStartOnLogin(checked)
                checked = Qt.binding(() => backend.startOnLogin)
            }
        }
    }
    Card {
        heading: qsTr("About")
        Controls.Label {
            Layout.fillWidth: true
            text: qsTr("App %1 · Daemon %2").arg(Qt.application.version).arg(backend.state.Version || "—")
            wrapMode: Text.WordWrap
        }
        Controls.Label {
            Layout.fillWidth: true
            text: qsTr("%1 · Model %2\nFirmware %3\nAudio backend: %4").arg(backend.state.ModelName || "AirPods").arg(backend.state.Model || "—").arg(backend.state.Firmware || "—").arg(backend.state.EqBackend || "—")
            wrapMode: Text.WordWrap
        }
        Controls.Button {
            text: qsTr("Releases and updates"); icon.name: "system-software-update"
            onClicked: Qt.openUrlExternally("https://github.com/superninjv/airpods-helper/releases")
        }
    }
}
