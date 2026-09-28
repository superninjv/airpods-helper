import app from "ags/gtk4/app"
import { Astal, Gtk, Gdk } from "ags/gtk4"
import Gtk4LayerShell from "gi://Gtk4LayerShell"
import GLib from "gi://GLib"
import { createState, With } from "gnim"
import { getState, isHeadphones, setConnectionCallbacks, type AirPodsState } from "./AirPodsService"

const [getVisible, setVisible] = createState(false)

let popupWindow: Gtk.Window | null = null
let dismissTimeout: number | null = null

const ANC_LABELS: Record<string, string> = {
  off: "ANC off",
  noise: "Noise Cancellation",
  transparency: "Transparency",
  adaptive: "Adaptive",
}

/** One battery row; level -1 (unknown / not reported) renders as "\u2014". */
function batteryBar(label: string, level: number, charging: boolean): Gtk.Widget {
  const known = level >= 0
  const color =
    !known ? "airpods-bat-unknown" :
    level <= 15 ? "airpods-bat-red" :
    level <= 30 ? "airpods-bat-yellow" :
    "airpods-bat-green"

  return (
    <box class={`airpods-bat-row ${color}`} spacing={6}>
      <label class="airpods-bat-label" label={label} widthChars={7} xalign={0} />
      <levelbar
        class="airpods-bat-bar"
        value={known ? level / 100 : 0}
        hexpand={true}
      />
      <label
        class="airpods-bat-pct"
        label={`${known ? `${level}%` : "\u2014"}${charging ? " \u26A1" : ""}`}
        widthChars={6}
        xalign={1}
      />
    </box>
  ) as Gtk.Widget
}

/** Rows for the current model: one "Battery" row for over-ear, else L/R/Case. */
function batteryRows(s: AirPodsState): Gtk.Widget {
  const rows = isHeadphones(s)
    ? [batteryBar("Battery", Math.max(s.batteryLeft, s.batteryRight), s.chargingLeft || s.chargingRight)]
    : [
        batteryBar("Left", s.batteryLeft, s.chargingLeft),
        batteryBar("Right", s.batteryRight, s.chargingRight),
        batteryBar("Case", s.batteryCase, s.chargingCase),
      ]
  const box = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 4 })
  for (const r of rows) box.append(r)
  return box as Gtk.Widget
}

function createPopupWindow(gdkmonitor: Gdk.Monitor): Gtk.Window {
  const win = new Gtk.Window()

  Gtk4LayerShell.init_for_window(win)
  Gtk4LayerShell.set_layer(win, Gtk4LayerShell.Layer.OVERLAY)
  Gtk4LayerShell.set_namespace(win, "airpods-popup")
  Gtk4LayerShell.set_monitor(win, gdkmonitor)
  Gtk4LayerShell.set_anchor(win, Gtk4LayerShell.Edge.BOTTOM, true)
  Gtk4LayerShell.set_margin(win, Gtk4LayerShell.Edge.BOTTOM, 24)
  Gtk4LayerShell.set_exclusive_zone(win, -1)

  win.add_css_class("AirPodsPopup")

  const revealer = new Gtk.Revealer({
    transitionType: Gtk.RevealerTransitionType.SLIDE_UP,
    transitionDuration: 350,
    revealChild: false,
  })

  const card = (
    <box class="airpods-card" orientation={Gtk.Orientation.VERTICAL} spacing={8}>
      <box class="airpods-card-header" spacing={10}>
        <label class="airpods-icon" label="\uF025" />
        <box orientation={Gtk.Orientation.VERTICAL}>
          <label
            class="airpods-model"
            label={getState.as((s) => s.modelName || s.model || "AirPods")}
            xalign={0}
          />
          <label
            class="airpods-status"
            label={getState.as((s) =>
              s.features.includes("anc") ? (ANC_LABELS[s.ancMode] ?? s.ancMode) : "Connected"
            )}
            xalign={0}
          />
        </box>
      </box>

      <box class="airpods-batteries" orientation={Gtk.Orientation.VERTICAL} spacing={4}>
        <With value={getState}>{(s: AirPodsState) => batteryRows(s)}</With>
      </box>
    </box>
  ) as Gtk.Widget

  revealer.set_child(card)
  win.set_child(revealer)

  // Click to dismiss
  const click = new Gtk.GestureClick()
  click.connect("pressed", () => dismissPopup())
  win.add_controller(click)

  // Bind visibility
  getVisible.subscribe(() => {
    const visible = getVisible()
    if (visible) {
      win.present()
      // Small delay for animation
      GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, () => {
        revealer.set_reveal_child(true)
        return GLib.SOURCE_REMOVE
      })
    } else {
      revealer.set_reveal_child(false)
      GLib.timeout_add(GLib.PRIORITY_DEFAULT, 400, () => {
        if (!getVisible()) win.set_visible(false)
        return GLib.SOURCE_REMOVE
      })
    }
  })

  return win
}

function showPopup() {
  if (dismissTimeout) {
    GLib.source_remove(dismissTimeout)
    dismissTimeout = null
  }

  setVisible(true)

  // Auto-dismiss after 5 seconds
  dismissTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 5000, () => {
    dismissPopup()
    dismissTimeout = null
    return GLib.SOURCE_REMOVE
  })
}

function dismissPopup() {
  if (dismissTimeout) {
    GLib.source_remove(dismissTimeout)
    dismissTimeout = null
  }
  setVisible(false)
}

export function initPopup(gdkmonitor: Gdk.Monitor) {
  popupWindow = createPopupWindow(gdkmonitor)

  // Register callbacks for device connection events
  setConnectionCallbacks(
    (_model: string) => showPopup(),
    () => dismissPopup(),
  )
}
