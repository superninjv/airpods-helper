import { Gtk } from "ags/gtk4"
import { execAsync } from "ags/process"
import GLib from "gi://GLib"
import Gio from "gi://Gio"
// Bar lock callbacks — set externally by host app
let _lockBar = () => {}
let _unlockBar = () => {}
export function setBarLock(lock: () => void, unlock: () => void) {
  _lockBar = lock
  _unlockBar = unlock
}

// ─── D-Bus proxy (inline, no gnim) ─────────────────────────────

const BUS = "org.costa.AirPods"
const PATH = "/org/costa/AirPods"

let proxy: Gio.DBusProxy | null = null
let state = {
  connected: false,
  batteryLeft: -1, batteryRight: -1, batteryCase: -1,
  chargingLeft: false, chargingRight: false, chargingCase: false,
  ancMode: "off",
  earLeft: false, earRight: false,
  conversationalAwareness: false,
  adaptiveNoiseLevel: 50,
  oneBudAnc: true,
  eqPreset: "",
  model: "", firmware: "",
}

const listeners: (() => void)[] = []
function notify() { for (const fn of listeners) fn() }

function gp(name: string): any {
  if (!proxy) return null
  const v = proxy.get_cached_property(name)
  return v ? v.deepUnpack() : null
}

function sync() {
  if (!proxy) {
    state = { ...state, connected: false }
    notify()
    return
  }
  state = {
    connected: gp("Connected") ?? false,
    batteryLeft: gp("BatteryLeft") ?? -1,
    batteryRight: gp("BatteryRight") ?? -1,
    batteryCase: gp("BatteryCase") ?? -1,
    chargingLeft: gp("ChargingLeft") ?? false,
    chargingRight: gp("ChargingRight") ?? false,
    chargingCase: gp("ChargingCase") ?? false,
    ancMode: gp("AncMode") ?? "off",
    earLeft: gp("EarLeft") ?? false,
    earRight: gp("EarRight") ?? false,
    conversationalAwareness: gp("ConversationalAwareness") ?? false,
    adaptiveNoiseLevel: gp("AdaptiveNoiseLevel") ?? 50,
    oneBudAnc: gp("OneBudAnc") ?? true,
    eqPreset: gp("EqPreset") ?? "",
    model: gp("Model") ?? "",
    firmware: gp("Firmware") ?? "",
  }
  notify()
}

function call(method: string, args: GLib.Variant | null = null) {
  if (!proxy) return
  proxy.call(method, args, Gio.DBusCallFlags.NONE, 5000, null, null)
}

function initProxy() {
  try {
    proxy = Gio.DBusProxy.new_for_bus_sync(Gio.BusType.SESSION, Gio.DBusProxyFlags.NONE, null, BUS, PATH, BUS, null)
    proxy.connect("g-properties-changed", () => sync())
    sync()
  } catch { proxy = null }
}

Gio.bus_watch_name(Gio.BusType.SESSION, BUS, Gio.BusNameWatcherFlags.NONE,
  () => initProxy(),
  () => { proxy = null; state = { ...state, connected: false }; notify() },
)
initProxy()

// ─── Helpers ────────────────────────────────────────────────────

function clearBox(box: Gtk.Box) {
  let c = box.get_first_child()
  while (c) { const n = c.get_next_sibling(); box.remove(c); c = n }
}

function batColor(level: number): string {
  if (level <= 15) return "ap-bat-red"
  if (level <= 30) return "ap-bat-yellow"
  return "ap-bat-green"
}

// ─── Widget ─────────────────────────────────────────────────────

export default function AirPodsBattery() {
  // Bar label
  const barLabel = new Gtk.Label({ label: "\uF025" })

  // ── Battery section ──
  function makeBatRow(label: string, level: number, charging: boolean): Gtk.Widget {
    const row = new Gtk.Box({ spacing: 8, cssClasses: ["ap-bat-row", batColor(level)] })
    row.append(new Gtk.Label({ label, widthChars: 5, xalign: 0, cssClasses: ["ap-bat-label"] }))
    const bar = new Gtk.LevelBar({ value: level / 100, hexpand: true, cssClasses: ["ap-bat-bar"] })
    row.append(bar)
    row.append(new Gtk.Label({ label: `${level}%${charging ? " \u26A1" : ""}`, widthChars: 6, xalign: 1, cssClasses: ["ap-bat-pct"] }))
    return row as Gtk.Widget
  }

  const batteryBox = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 4, cssClasses: ["ap-section"] })

  // ── ANC mode selector ──
  const ancModes = [
    { id: "off", label: "Off", icon: "\uF057" },
    { id: "noise", label: "ANC", icon: "\uF2A2" },
    { id: "transparency", label: "Transp.", icon: "\uF29C" },
    { id: "adaptive", label: "Adaptive", icon: "\uF042" },
  ]

  const ancBox = new Gtk.Box({ spacing: 4, cssClasses: ["ap-anc-row"], homogeneous: true })
  const ancBtns: Gtk.Button[] = []

  for (const mode of ancModes) {
    const btn = new Gtk.Button({ cssClasses: ["ap-anc-btn"], tooltipText: mode.label })
    const inner = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 2 })
    inner.append(new Gtk.Label({ label: mode.icon, cssClasses: ["ap-anc-icon"] }))
    inner.append(new Gtk.Label({ label: mode.label, cssClasses: ["ap-anc-label"] }))
    btn.set_child(inner)
    btn.connect("clicked", () => call("SetAncMode", new GLib.Variant("(s)", [mode.id])))
    ancBtns.push(btn)
    ancBox.append(btn)
  }

  // ── Adaptive noise level slider ──
  const noiseSliderBox = new Gtk.Box({ spacing: 8, cssClasses: ["ap-section", "ap-noise-row"], visible: false })
  noiseSliderBox.append(new Gtk.Label({ label: "Noise Level", hexpand: false, xalign: 0, cssClasses: ["ap-toggle-label"] }))
  const noiseSlider = new Gtk.Scale({
    orientation: Gtk.Orientation.HORIZONTAL,
    hexpand: true,
    cssClasses: ["ap-noise-slider"],
  })
  noiseSlider.set_range(0, 100)
  noiseSlider.set_value(50)
  noiseSlider.set_draw_value(false)
  let sliderTimeout: number | null = null
  noiseSlider.connect("value-changed", () => {
    if (sliderTimeout) GLib.source_remove(sliderTimeout)
    sliderTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 300, () => {
      call("SetAdaptiveNoiseLevel", new GLib.Variant("(y)", [Math.round(noiseSlider.get_value())]))
      sliderTimeout = null
      return GLib.SOURCE_REMOVE
    })
  })
  noiseSliderBox.append(noiseSlider)

  // ── Toggles ──
  const togglesBox = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 4, cssClasses: ["ap-section"] })

  const caRow = new Gtk.Box({ spacing: 8 })
  caRow.append(new Gtk.Label({ label: "Conversational Awareness", hexpand: true, xalign: 0, cssClasses: ["ap-toggle-label"] }))
  const caSwitch = new Gtk.Switch({ cssClasses: ["ap-toggle"] })
  caRow.append(caSwitch)

  const obRow = new Gtk.Box({ spacing: 8 })
  obRow.append(new Gtk.Label({ label: "One-Bud ANC", hexpand: true, xalign: 0, cssClasses: ["ap-toggle-label"] }))
  const obSwitch = new Gtk.Switch({ cssClasses: ["ap-toggle"] })
  obRow.append(obSwitch)

  togglesBox.append(caRow)
  togglesBox.append(obRow)

  // ── EQ section ──
  const eqBox = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 4, cssClasses: ["ap-section"] })
  const eqLabel = new Gtk.Label({ label: "Equalizer", xalign: 0, cssClasses: ["ap-section-title"] })
  const eqBtnBox = new Gtk.Box({ spacing: 4, cssClasses: ["ap-eq-row"] })
  eqBox.append(eqLabel)
  eqBox.append(eqBtnBox)

  // Load EQ presets from config dir
  const eqPresets = ["flat", "bass-boost", "vocal-clarity", "airpods-pro-crinacle"]
  const eqBtns: { name: string; btn: Gtk.Button }[] = []

  for (const preset of eqPresets) {
    const shortName = preset === "airpods-pro-crinacle" ? "Crinacle" :
      preset.split("-").map(w => w[0].toUpperCase() + w.slice(1)).join(" ")
    const btn = new Gtk.Button({ cssClasses: ["ap-eq-btn"], label: shortName })
    btn.connect("clicked", () => {
      if (state.eqPreset.toLowerCase().includes(preset)) {
        call("DisableEq")
      } else {
        call("SetEqPreset", new GLib.Variant("(s)", [preset]))
      }
    })
    eqBtns.push({ name: preset, btn })
    eqBtnBox.append(btn)
  }

  // ── Footer ──
  const footerLabel = new Gtk.Label({ cssClasses: ["ap-footer"], xalign: 0 })

  // ── Ear status ──
  const earBox = new Gtk.Box({ spacing: 8, cssClasses: ["ap-section", "ap-ear-row"] })
  const earLeftLabel = new Gtk.Label({ cssClasses: ["ap-ear"] })
  const earRightLabel = new Gtk.Label({ cssClasses: ["ap-ear"] })
  earBox.append(earLeftLabel)
  earBox.append(earRightLabel)

  // ── Assembly ──
  const popupBox = new Gtk.Box({
    orientation: Gtk.Orientation.VERTICAL,
    cssClasses: ["ap-popup"],
    widthRequest: 300,
    spacing: 8,
  })

  // Header
  const headerBox = new Gtk.Box({ spacing: 8, cssClasses: ["ap-header"] })
  const headerIcon = new Gtk.Label({ label: "\uF025", cssClasses: ["ap-header-icon"] })
  const headerTitle = new Gtk.Label({ label: "AirPods Pro", hexpand: true, xalign: 0, cssClasses: ["ap-header-title"] })
  const headerStatus = new Gtk.Label({ label: "", cssClasses: ["ap-header-status"] })
  headerBox.append(headerIcon)
  const headerText = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL })
  headerText.append(headerTitle)
  headerText.append(headerStatus)
  headerBox.append(headerText)

  popupBox.append(headerBox)
  popupBox.append(batteryBox)
  popupBox.append(ancBox)
  popupBox.append(noiseSliderBox)
  popupBox.append(togglesBox)
  popupBox.append(eqBox)
  popupBox.append(earBox)
  popupBox.append(footerLabel)

  // ── Render function ──
  let updatingToggles = false

  function render() {
    const s = state

    // Bar button
    if (s.connected) {
      const min = Math.min(
        s.batteryLeft >= 0 ? s.batteryLeft : 999,
        s.batteryRight >= 0 ? s.batteryRight : 999,
      )
      barLabel.label = min < 999 ? `\uF025 ${min}%` : "\uF025"
    }

    // Header
    headerTitle.label = s.model || "AirPods"
    headerStatus.label = s.ancMode === "off" ? "ANC Off" :
      s.ancMode === "noise" ? "Noise Cancellation" :
      s.ancMode === "transparency" ? "Transparency" :
      s.ancMode === "adaptive" ? "Adaptive" : s.ancMode

    // Battery
    clearBox(batteryBox)
    if (s.batteryLeft >= 0) batteryBox.append(makeBatRow("Left", s.batteryLeft, s.chargingLeft))
    if (s.batteryRight >= 0) batteryBox.append(makeBatRow("Right", s.batteryRight, s.chargingRight))
    if (s.batteryCase >= 0) batteryBox.append(makeBatRow("Case", s.batteryCase, s.chargingCase))

    // ANC buttons
    for (let i = 0; i < ancModes.length; i++) {
      const active = ancModes[i].id === s.ancMode
      ancBtns[i].cssClasses = active ? ["ap-anc-btn", "active"] : ["ap-anc-btn"]
    }

    // Adaptive noise slider
    noiseSliderBox.visible = s.ancMode === "adaptive"
    if (s.ancMode === "adaptive") {
      noiseSlider.set_value(s.adaptiveNoiseLevel)
    }

    // Toggles (block signal to avoid feedback loop)
    updatingToggles = true
    caSwitch.active = s.conversationalAwareness
    obSwitch.active = s.oneBudAnc
    updatingToggles = false

    // EQ
    for (const eq of eqBtns) {
      const active = s.eqPreset.toLowerCase().includes(eq.name)
      eq.btn.cssClasses = active ? ["ap-eq-btn", "active"] : ["ap-eq-btn"]
    }

    // Ears
    earLeftLabel.label = `L: ${s.earLeft ? "\uF58F In" : "\uF58E Out"}`
    earRightLabel.label = `R: ${s.earRight ? "\uF58F In" : "\uF58E Out"}`

    // Footer
    footerLabel.label = s.firmware ? `${s.model}  ·  FW ${s.firmware}` : ""
  }

  // Block toggle signals during programmatic updates
  caSwitch.connect("notify::active", () => { if (!updatingToggles) call("SetConversationalAwareness", new GLib.Variant("(b)", [caSwitch.active])) })
  obSwitch.connect("notify::active", () => { if (!updatingToggles) call("SetOneBudAnc", new GLib.Variant("(b)", [obSwitch.active])) })

  listeners.push(render)
  render()

  const popover = new Gtk.Popover()
  popover.set_child(popupBox)
  popover.connect("notify::visible", () => {
    if (popover.visible) { _lockBar(); sync() }
    else _unlockBar()
  })

  const menuBtn = new Gtk.MenuButton({
    cssClasses: ["airpods-btn"],
    popover: popover,
    visible: state.connected,
  })
  menuBtn.set_child(barLabel)

  listeners.push(() => {
    menuBtn.visible = state.connected
    const min = Math.min(
      state.batteryLeft >= 0 ? state.batteryLeft : 999,
      state.batteryRight >= 0 ? state.batteryRight : 999,
    )
    if (min <= 15) menuBtn.cssClasses = ["airpods-btn", "airpods-low"]
    else if (min <= 30) menuBtn.cssClasses = ["airpods-btn", "airpods-warn"]
    else menuBtn.cssClasses = ["airpods-btn"]
  })

  return menuBtn as Gtk.Widget
}
