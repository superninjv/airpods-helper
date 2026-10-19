import QtQuick
import QtQuick.Controls as QQC
import org.kde.plasma.components as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import "qrc:/shared/eq.js" as Eq

Card {
    id: equalizer
    heading: qsTr("Equalizer")
    property bool editing: false
    property bool newPreset: false
    property string draftId: ""
    property int revision: 0
    readonly property var s: backend.state
    readonly property string selectedId: Eq.AirPodsEq.selectedPresetId(s.EqPreset, backend.presets)
    readonly property var selected: backend.presets.find(p => p.id === selectedId) || ({})
    readonly property var preview: {
        revision
        if (!editing) return selectedId && backend.eqDetail.id === selectedId ? backend.eqDetail : null
        const bands = []
        for (let i = 0; i < bandRows.count; ++i) {
            const b = bandRows.get(i)
            bands.push({type: b.filterType, freq: b.freq, q: b.q, gain: b.gain})
        }
        return {name: presetName.text, preamp: preamp.value / 10, bands: bands}
    }
    signal editRequested()
    signal finished()
    function startEditor(fresh) {
        if (!fresh && backend.eqDetail.id !== selectedId) return
        const detail = fresh ? {name: qsTr("My preset"), description: "", preamp: 0, bands: []} : backend.eqDetail
        newPreset = fresh || !selected.user_editable
        draftId = newPreset ? "" : detail.id
        presetName.text = detail.name + (!fresh && newPreset ? qsTr(" copy") : "")
        description.text = detail.description
        preamp.value = Math.round(detail.preamp * 10)
        bandRows.clear()
        for (const b of detail.bands) bandRows.append({filterType: b.type, freq: b.freq, q: b.q, gain: b.gain})
        editing = true
        revision++
        editRequested()
    }
    function save() {
        let id = draftId
        if (newPreset) {
            const base = presetName.text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40) || "my-preset"
            id = base
            for (let n = 2; backend.presets.some(p => p.id === id); ++n) id = base + "-" + n
        }
        backend.saveEqPreset({id: id, name: presetName.text, description: description.text, preamp: preview.preamp, bands: preview.bands}, newPreset)
    }
    ListModel { id: bandRows }
    Flow {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
        Repeater {
            model: [{id: "", name: qsTr("Off")}].concat(Eq.AirPodsEq.visiblePresets(backend.presets))
            delegate: Controls.Button {
                required property var modelData
                objectName: "eq-" + modelData.id
                text: Eq.AirPodsEq.presetLabel(modelData)
                font: Kirigami.Theme.smallFont
                checkable: true
                checked: (equalizer.selectedId || "") === modelData.id
                enabled: !equalizer.editing && !backend.eqBusy && (equalizer.s.EqStatus !== "unsupported" || modelData.id === "")
                Accessible.role: Accessible.RadioButton
                Controls.ToolTip.visible: hovered
                Controls.ToolTip.text: modelData.name + (modelData.description ? " · " + modelData.description : "")
                onClicked: {
                    backend.control(modelData.id ? "SetEqPreset" : "DisableEq", modelData.id)
                    checked = Qt.binding(() => (equalizer.selectedId || "") === modelData.id)
                }
            }
        }
    }
    Controls.Label {
        objectName: "eq-status"
        Layout.fillWidth: true
        text: equalizer.s.EqError || (!equalizer.selectedId ? qsTr("Equalizer off") : equalizer.s.EqStatus === "active" ? (equalizer.s.EqBackend === "pipewire-legacy" ? qsTr("Older WirePlumber: the EQ is your default output while on.") : qsTr("Active · %1").arg(equalizer.s.EqBackend || "")) : equalizer.s.EqStatus === "waiting" ? qsTr("Waiting for your AirPods audio output") : equalizer.s.EqStatus === "unsupported" ? qsTr("No supported PipeWire or PulseAudio server was found") : qsTr("Equalizer off"))
        wrapMode: Text.WordWrap
        color: equalizer.s.EqError ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
    }
    EqCurve { objectName: "eq-curve"; preset: equalizer.preview }
    Controls.Label {
        Layout.fillWidth: true
        text: equalizer.preview ? qsTr("%1 · %2 bands · Preamp %3 dB").arg(equalizer.editing ? qsTr("Preview") : equalizer.preview.name).arg(equalizer.preview.bands.length).arg(equalizer.preview.preamp) : equalizer.selectedId ? qsTr("Loading preset…") : qsTr("Flat response")
        wrapMode: Text.WordWrap
    }
    Controls.Label {
        visible: !equalizer.editing && text !== ""
        Layout.fillWidth: true
        text: equalizer.selected.description || ""
        wrapMode: Text.WordWrap
    }
    Flow {
        visible: !equalizer.editing
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
        Controls.Button { objectName: "eq-new"; text: qsTr("New preset"); enabled: !backend.eqBusy; onClicked: equalizer.startEditor(true) }
        Controls.Button {
            objectName: "eq-edit"
            visible: !!equalizer.selectedId
            enabled: !backend.eqBusy && backend.eqDetail.id === equalizer.selectedId
            text: equalizer.selected.user_editable ? qsTr("Edit") : qsTr("Customize")
            onClicked: equalizer.startEditor(false)
        }
    }
    ColumnLayout {
        visible: equalizer.editing
        Layout.fillWidth: true
        enabled: !backend.eqBusy
        Controls.Label { text: qsTr("Preset name") }
        Controls.TextField {
            id: presetName
            objectName: "eq-name"
            Layout.fillWidth: true
            maximumLength: 64
            Accessible.name: qsTr("Preset name")
        }
        Controls.Label { text: qsTr("Description") }
        Controls.TextField {
            id: description
            objectName: "eq-description"
            Layout.fillWidth: true
            maximumLength: 256
            Accessible.name: qsTr("Preset description")
        }
        RowLayout {
            Controls.Label { text: qsTr("Preamp (dB)"); Layout.fillWidth: true }
            Controls.SpinBox {
                id: preamp
                objectName: "eq-preamp"
                from: -240; to: 120; stepSize: 5; editable: true
                textFromValue: (value, locale) => Number(value / 10).toLocaleString(locale, 'f', 1)
                valueFromText: (text, locale) => Math.round(Number.fromLocaleString(locale, text) * 10)
                validator: DoubleValidator { bottom: -24; top: 12; decimals: 1; locale: preamp.locale.name }
                Accessible.name: qsTr("Preamp in decibels")
            }
        }
        Repeater {
            model: bandRows
            delegate: ColumnLayout {
                id: band
                required property int index
                required property string filterType
                required property real freq
                required property real q
                required property real gain
                Layout.fillWidth: true
                Kirigami.Separator { Layout.fillWidth: true }
                RowLayout {
                    Controls.Label { text: qsTr("Band %1").arg(band.index + 1); Layout.fillWidth: true }
                    Controls.Button { text: qsTr("Remove"); onClicked: { bandRows.remove(band.index); equalizer.revision++ } }
                }
                Controls.ComboBox {
                            flat: true
                    Layout.fillWidth: true
                    model: Eq.AirPodsEq.FILTER_TYPES.map(t => ({value: t[0], text: t[1]}))
                    textRole: "text"; valueRole: "value"
                    currentIndex: Eq.AirPodsEq.FILTER_TYPES.findIndex(t => t[0] === band.filterType)
                    onActivated: { bandRows.setProperty(band.index, "filterType", currentValue); equalizer.revision++ }
                    Accessible.name: qsTr("Band %1 filter type").arg(band.index + 1)
                }
                RowLayout {
                    Controls.Label { text: qsTr("Frequency (Hz)"); Layout.fillWidth: true }
                    Controls.SpinBox {
                        from: 20; to: 20000; value: Math.round(band.freq); editable: true
                        onValueModified: { bandRows.setProperty(band.index, "freq", value); equalizer.revision++ }
                        Accessible.name: qsTr("Band %1 frequency in Hz").arg(band.index + 1)
                    }
                }
                RowLayout {
                    Controls.Label { text: qsTr("Q"); Layout.fillWidth: true }
                    Controls.SpinBox {
                        id: qInput
                        from: 10; to: 1000; stepSize: 10; value: Math.round(band.q * 100); editable: true
                        textFromValue: (value, locale) => Number(value / 100).toLocaleString(locale, 'f', 2)
                        valueFromText: (text, locale) => Math.round(Number.fromLocaleString(locale, text) * 100)
                        validator: DoubleValidator { bottom: 0.1; top: 10; decimals: 2; locale: qInput.locale.name }
                        onValueModified: { bandRows.setProperty(band.index, "q", value / 100); equalizer.revision++ }
                        Accessible.name: qsTr("Band %1 Q").arg(band.index + 1)
                    }
                }
                RowLayout {
                    Controls.Label { text: qsTr("Gain (dB)"); Layout.fillWidth: true }
                    Controls.SpinBox {
                        id: gainInput
                        from: -240; to: 240; stepSize: 5; value: Math.round(band.gain * 10); editable: true
                        enabled: ["peaking", "lowshelf", "highshelf"].indexOf(band.filterType) !== -1
                        textFromValue: (value, locale) => Number(value / 10).toLocaleString(locale, 'f', 1)
                        valueFromText: (text, locale) => Math.round(Number.fromLocaleString(locale, text) * 10)
                        validator: DoubleValidator { bottom: -24; top: 24; decimals: 1; locale: gainInput.locale.name }
                        onValueModified: { bandRows.setProperty(band.index, "gain", value / 10); equalizer.revision++ }
                        Accessible.name: qsTr("Band %1 gain in decibels").arg(band.index + 1)
                    }
                }
            }
        }
        Controls.Button {
            objectName: "eq-add-band"
            text: qsTr("Add band")
            enabled: bandRows.count < 16
            onClicked: { bandRows.append({filterType: "peaking", freq: 1000, q: 1, gain: 0}); equalizer.revision++ }
        }
        Controls.Label { text: qsTr("Up to 16 bands. The curve previews the sound shape; preamp controls overall headroom."); wrapMode: Text.WordWrap; Layout.fillWidth: true }
        Flow {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            Controls.Button {
                objectName: "eq-save"
                text: equalizer.newPreset ? qsTr("Save and use") : qsTr("Save")
                enabled: presetName.text.trim() !== ""
                onClicked: equalizer.save()
            }
            Controls.Button {
                text: qsTr("Cancel")
                onClicked: { equalizer.editing = false; equalizer.finished() }
            }
            Controls.Button {
                objectName: "eq-delete"
                text: qsTr("Delete")
                visible: !equalizer.newPreset
                onClicked: deleteDialog.open()
            }
        }
    }
    Controls.Dialog {
        id: deleteDialog
        objectName: "eq-delete-dialog"
        title: qsTr("Delete preset?")
        modal: true
        anchors.centerIn: QQC.Overlay.overlay
        standardButtons: Controls.Dialog.Yes | Controls.Dialog.No
        Controls.Label { text: qsTr("Delete “%1”? This cannot be undone.").arg(presetName.text); wrapMode: Text.WordWrap; width: 280 }
        onAccepted: backend.deleteEqPreset(equalizer.draftId)
    }
    Connections {
        target: backend
        function onEqSaved(id) { equalizer.editing = false; equalizer.finished() }
        function onEqDeleted(id) { equalizer.editing = false; equalizer.finished() }
    }
}
