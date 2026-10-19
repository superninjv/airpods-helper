import QtQuick
import org.kde.plasma.components as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: devices
    property string renamingAddress: ""
    readonly property var preferredDevice: backend.devices.find(device => device.address.toUpperCase() === (backend.state.PreferredDevice || "").toUpperCase())
    Connections { target: backend; function onDeviceRenamed() { devices.renamingAddress = "" } }
    Layout.fillWidth: true
    property bool busy: backend.deviceOperation !== ""
    Card {
        heading: qsTr("Devices")
        objectName: "devices-page"
        Controls.Label {
            Layout.fillWidth: true
            text: qsTr("Open the case and hold its button until the light flashes white to pair your AirPods.")
            wrapMode: Text.WordWrap
        }
        RowLayout {
            Controls.Button {
                text: backend.scanning ? qsTr("Scanning…") : qsTr("Find AirPods")
                enabled: backend.available && !backend.scanning
                onClicked: backend.scan()
            }
            Controls.Button {
                text: qsTr("Disconnect")
                visible: !!backend.state.Connected
                enabled: !busy
                onClicked: backend.control("Disconnect")
            }
        }
        Controls.Label {
            visible: busy
            text: qsTr("%1…").arg(backend.deviceOperation)
            Accessible.role: Accessible.AlertMessage
        }
    }
    Card {
        heading: qsTr("Paired devices")
        Controls.Label { visible: backend.devices.length === 0; text: qsTr("No paired AirPods found.") }
        Repeater {
            model: backend.devices
            delegate: ColumnLayout {
                id: pairedDevice
                required property var modelData
                Layout.fillWidth: true
                Item {
                    id: deviceTitle
                    Layout.fillWidth: true
                    implicitHeight: Math.max(pairedName.implicitHeight, preferredBadge.implicitHeight)
                    Controls.Label {
                        id: pairedName
                        objectName: "device-name-" + pairedDevice.modelData.address
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        width: Math.min(implicitWidth, Math.max(0, deviceTitle.width - (preferredBadge.visible ? preferredBadge.implicitWidth + Kirigami.Units.smallSpacing : 0)))
                        text: pairedDevice.modelData.name || qsTr("AirPods")
                        elide: Text.ElideRight
                    }
                    Controls.Label {
                        id: preferredBadge
                        anchors.left: pairedName.right
                        anchors.leftMargin: Kirigami.Units.smallSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        objectName: "device-preferred-" + pairedDevice.modelData.address
                        visible: pairedDevice.modelData.address.toUpperCase() === (backend.state.PreferredDevice || "").toUpperCase()
                        text: qsTr("Preferred")
                        font: Kirigami.Theme.smallFont
                        color: Kirigami.Theme.highlightColor
                    }
                }
                Controls.Label {
                    Layout.fillWidth: true
                    text: pairedDevice.modelData.address
                    font: Kirigami.Theme.smallFont
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
                }
                RowLayout {
                    Controls.Button {
                        text: qsTr("Connect")
                        enabled: backend.available && !devices.busy
                        onClicked: backend.control("ConnectTo", pairedDevice.modelData.address)
                    }
                    Controls.Button {
                        objectName: "rename-" + pairedDevice.modelData.address
                        text: qsTr("Rename")
                        enabled: backend.available && !devices.busy
                        onClicked: {
                            renameName.text = pairedDevice.modelData.name || ""
                            devices.renamingAddress = pairedDevice.modelData.address
                            renameName.forceActiveFocus()
                        }
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    visible: devices.renamingAddress === pairedDevice.modelData.address
                    Controls.Label {
                        Layout.fillWidth: true
                        text: qsTr("Device name on this computer")
                    }
                    Controls.TextField {
                        id: renameName
                        objectName: "rename-name-" + pairedDevice.modelData.address
                        Layout.fillWidth: true
                        maximumLength: 64
                        Accessible.name: qsTr("Device name on this computer")
                        onAccepted: if (text.trim() && !devices.busy) backend.renameDevice(pairedDevice.modelData.address, text)
                    }
                    RowLayout {
                        Controls.Button {
                            objectName: "rename-save-" + pairedDevice.modelData.address
                            text: qsTr("Save")
                            enabled: backend.available && !devices.busy && renameName.text.trim() !== ""
                            onClicked: backend.renameDevice(pairedDevice.modelData.address, renameName.text)
                        }
                        Controls.Button { text: qsTr("Cancel"); onClicked: devices.renamingAddress = "" }
                    }
                }
            }
        }
        Controls.Button { text: qsTr("Refresh devices"); enabled: backend.available; onClicked: backend.refresh() }
    }
    Card {
        heading: qsTr("Nearby AirPods")
        visible: backend.scanning || backend.candidates.length > 0
        Controls.BusyIndicator { running: backend.scanning; visible: running; Layout.alignment: Qt.AlignHCenter }
        Repeater {
            model: backend.candidates
            delegate: ColumnLayout {
                required property var modelData
                Layout.fillWidth: true
                Controls.Label {
                    Layout.fillWidth: true
                    text: (modelData.name || modelData.model || qsTr("AirPods")) + " · " + modelData.rssi + " dBm"
                    wrapMode: Text.WordWrap
                }
                Controls.Label {
                    Layout.fillWidth: true
                    text: modelData.address + (modelData.in_pair_mode ? qsTr(" · Ready to pair") : qsTr(" · Open the case to pair"))
                    wrapMode: Text.WordWrap
                }
                Controls.Button {
                    text: qsTr("Pair and connect")
                    enabled: backend.available && !busy
                    onClicked: backend.control("Pair", modelData.address)
                }
            }
        }
    }
    Card {
        heading: qsTr("Pair by Bluetooth address")
        Controls.TextField {
            id: address
            objectName: "pair-address"
            Layout.fillWidth: true
            placeholderText: "AA:BB:CC:DD:EE:FF"
            Accessible.name: qsTr("Bluetooth address")
            maximumLength: 17
            validator: RegularExpressionValidator { regularExpression: /[0-9a-fA-F]{2}(:[0-9a-fA-F]{2}){5}/ }
            onAccepted: if (acceptableInput && backend.available && !busy) backend.control("Pair", text)
        }
        Controls.Button {
            text: qsTr("Pair and connect")
            enabled: address.acceptableInput && backend.available && !busy
            onClicked: backend.control("Pair", address.text)
        }
    }
    Card {
        objectName: "preferred-device-card"
        heading: qsTr("Preferred device")
        Controls.Label {
            Layout.fillWidth: true
            text: qsTr("Use this Bluetooth address for automatic reconnection. Leave empty to use any paired AirPods.")
            wrapMode: Text.WordWrap
        }
        Controls.ComboBox {
            id: preferredPicker
            objectName: "preferred-picker"
            flat: true
            Layout.fillWidth: true
            model: [{name: qsTr("Any paired AirPods"), address: ""}].concat(Array.from(backend.devices))
            textRole: "name"
            valueRole: "address"
            currentIndex: model.findIndex(device => device.address.toUpperCase() === (backend.state.PreferredDevice || "").toUpperCase())
            enabled: backend.available
            Accessible.name: qsTr("Preferred device")
            contentItem: Item {
              implicitHeight: preferredRow.implicitHeight
              RowLayout {
                id: preferredRow
                anchors.fill: parent
                anchors.rightMargin: preferredPicker.indicator.width + preferredPicker.spacing
                Controls.Label {
                    objectName: "preferred-name"
                    Layout.fillWidth: true
                    text: devices.preferredDevice ? devices.preferredDevice.name : backend.state.PreferredDevice ? qsTr("AirPods") : qsTr("Any paired AirPods")
                    elide: Text.ElideRight
                }
                Controls.Label {
                    objectName: "preferred-address"
                    text: backend.state.PreferredDevice || ""
                    font: Kirigami.Theme.smallFont
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
                }
              }
            }
            delegate: Controls.ItemDelegate {
                required property var modelData
                width: preferredPicker.popup.availableWidth
                contentItem: RowLayout {
                    Controls.Label { Layout.fillWidth: true; text: modelData.name; elide: Text.ElideRight }
                    Controls.Label { text: modelData.address; font: Kirigami.Theme.smallFont; color: Qt.alpha(Kirigami.Theme.textColor, 0.6) }
                }
            }
            onActivated: {
                backend.setting("PreferredDevice", currentValue)
                currentIndex = Qt.binding(() => model.findIndex(device => device.address.toUpperCase() === (backend.state.PreferredDevice || "").toUpperCase()))
            }
        }
        Controls.ToolButton {
            id: manualPreferred
            text: qsTr("Enter a Bluetooth address…")
            checkable: true
        }
        Controls.TextField {
            id: preferred
            visible: manualPreferred.checked || !!backend.state.PreferredDevice && !devices.preferredDevice
            objectName: "preferred-device"
            Layout.fillWidth: true
            text: backend.state.PreferredDevice || ""
            placeholderText: "AA:BB:CC:DD:EE:FF"
            maximumLength: 17
            Accessible.name: qsTr("Preferred Bluetooth address")
            validator: RegularExpressionValidator { regularExpression: /([0-9a-fA-F]{2}(:[0-9a-fA-F]{2}){5})?/ }
            onEditingFinished: if (acceptableInput && backend.available && text.toUpperCase() !== (backend.state.PreferredDevice || "").toUpperCase()) backend.setting("PreferredDevice", text)
        }
        RowLayout {
            visible: preferred.visible
            Controls.Button { text: qsTr("Clear"); enabled: backend.available; onClicked: { preferred.clear(); backend.setting("PreferredDevice", "") } }
        }
    }

}
