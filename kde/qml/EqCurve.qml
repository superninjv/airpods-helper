import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as Controls
import org.kde.kirigami as Kirigami
import "qrc:/shared/eq.js" as Eq

Item {
    id: curve
    property var preset: null
    readonly property var response: Eq.AirPodsEq.response(preset)
    readonly property real range: Math.max(12, Math.ceil(response.reduce((m, p) => Math.max(m, Math.abs(p[1])), 0) / 6) * 6)
    Layout.fillWidth: true
    Layout.minimumWidth: 0
    readonly property real labelAngle: 60
    readonly property real labelSin: Math.sin(labelAngle * Math.PI / 180)
    readonly property real labelCos: Math.cos(labelAngle * Math.PI / 180)
    readonly property real axisLabelHeight: labelMetrics.advanceWidth("20 kHz") * labelSin + labelMetrics.height * labelCos
    implicitHeight: Math.max(Kirigami.Units.gridUnit * 8, labelMetrics.height * 6) + axisLabelHeight
    FontMetrics { id: labelMetrics; font: Kirigami.Theme.smallFont }
    Accessible.role: Accessible.Graphic
    Accessible.name: qsTr("Equalizer frequency response, 20 Hz to 20 kHz")
    Accessible.description: preset ? qsTr("%1, %2 bands, preamp %3 dB").arg(preset.name || qsTr("Preview")).arg(preset.bands.length).arg(preset.preamp) : qsTr("Equalizer off: flat response")

    Canvas {
        id: plot
        objectName: "eq-plot"
        anchors.fill: parent
        anchors.leftMargin: labelMetrics.advanceWidth(String(-curve.range) + " dB") + Kirigami.Units.smallSpacing
        anchors.rightMargin: labelMetrics.advanceWidth("20 kHz") * curve.labelCos + labelMetrics.height * curve.labelSin + Kirigami.Units.smallSpacing
        anchors.topMargin: labelMetrics.height / 2
        anchors.bottomMargin: curve.axisLabelHeight + Kirigami.Units.smallSpacing
        canvasSize: Qt.size(width, height)
        clip: true
        readonly property color ink: Kirigami.Theme.textColor
        readonly property color accent: Kirigami.Theme.highlightColor
        function frequencyX(f) { return Math.max(1, Math.min(width - 1, Math.log10(f / 20) / 3 * (width - 2) + 1)) }
        function gainY(db) { return 1 + (curve.range - db) / (curve.range * 2) * (height - 2) }
        onInkChanged: requestPaint()
        onAccentChanged: requestPaint()
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        Connections { target: curve; function onResponseChanged() { plot.requestPaint() } }
        onPaint: {
            const ctx = getContext("2d")
            ctx.reset()
            ctx.lineWidth = 1
            ctx.strokeStyle = Qt.alpha(ink, 0.15)
            ctx.strokeRect(1, 1, width - 2, height - 2)
            ctx.beginPath(); ctx.moveTo(1, gainY(0)); ctx.lineTo(width - 1, gainY(0)); ctx.stroke()
            for (const f of [100, 1000, 10000]) {
                ctx.beginPath(); ctx.moveTo(frequencyX(f), 0); ctx.lineTo(frequencyX(f), height); ctx.stroke()
            }
            ctx.strokeStyle = curve.preset ? accent : Qt.alpha(ink, 0.5)
            ctx.lineWidth = 2
            ctx.beginPath()
            curve.response.forEach((p, i) => {
                if (i === 0) ctx.moveTo(frequencyX(p[0]), gainY(p[1]))
                else ctx.lineTo(frequencyX(p[0]), gainY(p[1]))
            })
            ctx.stroke()
        }
    }
    Repeater {
        model: [-curve.range, 0, curve.range]
        Controls.Label {
            required property real modelData
            objectName: "eq-gain-" + modelData
            text: modelData + " dB"
            font: labelMetrics.font
            color: Qt.alpha(Kirigami.Theme.textColor, 0.65)
            y: plot.y + plot.gainY(modelData) - height / 2
        }
    }
    Repeater {
        model: [20, 100, 1000, 10000, 20000]
        Controls.Label {
            required property real modelData
            objectName: "eq-frequency-" + modelData
            text: Eq.AirPodsEq.formatHz(modelData)
            Accessible.name: text
            font: labelMetrics.font
            color: Qt.alpha(Kirigami.Theme.textColor, 0.65)
            transformOrigin: Item.TopLeft
            rotation: curve.labelAngle
            x: plot.x + plot.frequencyX(modelData) + height * curve.labelSin
            y: plot.y + plot.height + Kirigami.Units.smallSpacing
        }
    }
}
