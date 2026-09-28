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

// ─── D-Bus proxy (inline, no gnim) — API v2, see docs/dbus-api.md ──────

const BUS = "org.costa.AirPods"
const PATH = "/org/costa/AirPods"

interface EqPresetInfo {
  id: string
  name: string
  description: string
  userEditable: boolean
}

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
  micMode: "auto",
  eqPreset: "",
  eqStatus: "off",
  eqError: "",
  model: "", modelName: "", firmware: "",
  features: [] as string[],
}
let eqPresets: EqPresetInfo[] = []

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
    micMode: gp("MicMode") ?? "auto",
    eqPreset: gp("EqPreset") ?? "",
    eqStatus: gp("EqStatus") ?? "off",
    eqError: gp("EqError") ?? "",
    model: gp("Model") ?? "",
    modelName: gp("ModelName") ?? "",
    firmware: gp("Firmware") ?? "",
    features: gp("Features") ?? [],
  }
  // A preset we don't know yet (created via app/CLI) → refresh the list.
  if (state.eqPreset && !eqPresets.some((p) => p.id === state.eqPreset)) loadPresets()
  notify()
}

function call(method: string, args: GLib.Variant | null = null) {
  if (!proxy) return
  proxy.call(method, args, Gio.DBusCallFlags.NONE, 5000, null, (p, res) => {
    try {
      p!.call_finish(res)
    } catch (e) {
      console.error(`airpods: ${method} failed: ${e}`)
    }
  })
}

/** GetEqPresets → a(sssb); falls back to ListEqPresets (as) on v1 daemons. */
let presetsLoading = false
function loadPresets() {
  if (!proxy || presetsLoading) return
  presetsLoading = true
  proxy.call("GetEqPresets", null, Gio.DBusCallFlags.NONE, 5000, null, (p, res) => {
    try {
      const [list] = p!.call_finish(res).deepUnpack() as [[string, string, string, boolean][]]
      eqPresets = list.map(([id, name, description, userEditable]) => ({ id, name, description, userEditable }))
      presetsLoading = false
      notify()
    } catch {
      p!.call("ListEqPresets", null, Gio.DBusCallFlags.NONE, 5000, null, (p2, res2) => {
        presetsLoading = false
        try {
          const [ids] = p2!.call_finish(res2).deepUnpack() as [string[]]
          eqPresets = ids.map((id) => ({ id, name: presetLabel(id), description: "", userEditable: false }))
          notify()
        } catch (e) {
          console.error(`airpods: listing EQ presets failed: ${e}`)
        }
      })
    }
  })
}

function initProxy() {
  try {
    proxy = Gio.DBusProxy.new_for_bus_sync(Gio.BusType.SESSION, Gio.DBusProxyFlags.NONE, null, BUS, PATH, BUS, null)
    proxy.connect("g-properties-changed", () => sync())
    sync()
    loadPresets()
  } catch { proxy = null }
}

Gio.bus_watch_name(Gio.BusType.SESSION, BUS, Gio.BusNameWatcherFlags.NONE,
  () => initProxy(),
  () => { proxy = null; state = { ...state, connected: false }; notify() },
)
initProxy()

// ─── Helpers ────────────────────────────────────────────────────

function clearBox(box: Gtk.Box | Gtk.FlowBox) {
  let c = box.get_first_child()
  while (c) { const n = c.get_next_sibling(); box.remove(c); c = n }
}

function batColor(level: number): string {
  if (level < 0) return "ap-bat-unknown"
  if (level <= 15) return "ap-bat-red"
  if (level <= 30) return "ap-bat-yellow"
  return "ap-bat-green"
}

/** "bass-boost" → "Bass Boost" (used when the daemon has no display name). */
function presetLabel(id: string): string {
  return id.split(/[-_]/).filter(Boolean).map((w) => w[0].toUpperCase() + w.slice(1)).join(" ")
}

const ANC_LABELS: Record<string, string> = {
  off: "ANC Off",
  noise: "Noise Cancellation",
  transparency: "Transparency",
  adaptive: "Adaptive",
}

const EQ_STATUS_LABELS: Record<string, string> = {
  off: "Off",
  active: "Active",
  waiting: "Waiting",
  error: "Error",
  unsupported: "Unavailable",
}

// ─── Widget ─────────────────────────────────────────────────────

export default function AirPodsBattery() {
  // Bar label
  const barLabel = new Gtk.Label({ label: "" })

  // ── Battery section ──
  function makeBatRow(label: string, level: number, charging: boolean): Gtk.Widget {
    const known = level >= 0
    const row = new Gtk.Box({ spacing: 8, cssClasses: ["ap-bat-row", batColor(level)] })
    row.append(new Gtk.Label({ label, widthChars: 7, xalign: 0, cssClasses: ["ap-bat-label"] }))
    const bar = new Gtk.LevelBar({ value: known ? level / 100 : 0, hexpand: true, cssClasses: ["ap-bat-bar"] })
    row.append(bar)
    const text = known ? `${level}%` : "—"
    row.append(new Gtk.Label({ label: `${text}${charging ? " ⚡" : ""}`, widthChars: 6, xalign: 1, cssClasses: ["ap-bat-pct"] }))
    return row as Gtk.Widget
  }

  const batteryBox = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 4, cssClasses: ["ap-section"] })

  // ── ANC mode selector ──
  const ancModes = [
    { id: "off", label: "Off", icon: "" },
    { id: "noise", label: "ANC", icon: "" },
    { id: "transparency", label: "Transp.", icon: "" },
    { id: "adaptive", label: "Adaptive", icon: "" },
  ]

  const ancBox = new Gtk.Box({ spacing: 4, cssClasses: ["ap-anc-row"], homogeneous: true })
  const ancBtns: Gtk.Button[] = []

  for (const mode of ancModes) {
    const btn = new Gtk.Button({ cssClasses: ["ap-anc-btn"], tooltipText: ANC_LABELS[mode.id] })
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
  let updatingSlider = false
  noiseSlider.connect("value-changed", () => {
    if (updatingSlider) return
    if (sliderTimeout) GLib.source_remove(sliderTimeout)
    sliderTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 300, () => {
      call("SetAdaptiveNoiseLevel", new GLib.Variant("(y)", [Math.round(noiseSlider.get_value())]))
      sliderTimeout = null
      return GLib.SOURCE_REMOVE
    })
  })
  noiseSliderBox.append(noiseSlider)

  // ── Toggles + mic mode ──
  const togglesBox = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 4, cssClasses: ["ap-section"] })

  const caRow = new Gtk.Box({ spacing: 8 })
  caRow.append(new Gtk.Label({ label: "Conversational Awareness", hexpand: true, xalign: 0, cssClasses: ["ap-toggle-label"] }))
  const caSwitch = new Gtk.Switch({ cssClasses: ["ap-toggle"] })
  caRow.append(caSwitch)

  const obRow = new Gtk.Box({ spacing: 8 })
  obRow.append(new Gtk.Label({ label: "One-Bud ANC", hexpand: true, xalign: 0, cssClasses: ["ap-toggle-label"] }))
  const obSwitch = new Gtk.Switch({ cssClasses: ["ap-toggle"] })
  obRow.append(obSwitch)

  // Primary microphone (firmware doesn't report it; the daemon remembers
  // the last value a client set).
  const micRow = new Gtk.Box({ spacing: 8, cssClasses: ["ap-mic-row"] })
  micRow.append(new Gtk.Label({ label: "Microphone", hexpand: true, xalign: 0, cssClasses: ["ap-toggle-label"] }))
  const micModes = [
    { id: "auto", label: "Auto" },
    { id: "left", label: "Left" },
    { id: "right", label: "Right" },
  ]
  const micBtns: Gtk.Button[] = []
  const micBtnBox = new Gtk.Box({ spacing: 4 })
  for (const m of micModes) {
    const btn = new Gtk.Button({ label: m.label, cssClasses: ["ap-eq-btn", "ap-mic-btn"] })
    btn.connect("clicked", () => call("SetMicMode", new GLib.Variant("(s)", [m.id])))
    micBtns.push(btn)
    micBtnBox.append(btn)
  }
  micRow.append(micBtnBox)

  togglesBox.append(caRow)
  togglesBox.append(obRow)
  togglesBox.append(micRow)

  // ── EQ section ──
  const eqBox = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 4, cssClasses: ["ap-section"] })
  const eqHeader = new Gtk.Box({ spacing: 8 })
  eqHeader.append(new Gtk.Label({ label: "Equalizer", hexpand: true, xalign: 0, cssClasses: ["ap-section-title"] }))
  const eqStatusLabel = new Gtk.Label({ label: "", xalign: 1, cssClasses: ["ap-eq-status"] })
  eqHeader.append(eqStatusLabel)
  const eqFlow = new Gtk.FlowBox({
    selectionMode: Gtk.SelectionMode.NONE,
    columnSpacing: 4,
    rowSpacing: 4,
    maxChildrenPerLine: 4,
    homogeneous: false,
    cssClasses: ["ap-eq-row"],
  })
  const eqMsg = new Gtk.Label({ label: "", wrap: true, xalign: 0, maxWidthChars: 38, cssClasses: ["ap-eq-msg"], visible: false })
  eqBox.append(eqHeader)
  eqBox.append(eqFlow)
  eqBox.append(eqMsg)

  const eqBtns: { id: string; btn: Gtk.Button }[] = []
  let eqKey = ""

  function rebuildEqButtons() {
    const key = eqPresets.map((p) => `${p.id}\u001f${p.name}`).join("\u001e")
    if (key === eqKey) return
    eqKey = key
    clearBox(eqFlow)
    eqBtns.length = 0
    const items = [{ id: "", name: "Off", description: "Turn the equalizer off" }, ...eqPresets]
    for (const preset of items) {
      const btn = new Gtk.Button({
        cssClasses: ["ap-eq-btn"],
        label: preset.name || presetLabel(preset.id),
        tooltipText: preset.description || null,
      })
      btn.connect("clicked", () => {
        if (preset.id === "" || preset.id === state.eqPreset) call("DisableEq")
        else call("SetEqPreset", new GLib.Variant("(s)", [preset.id]))
      })
      eqBtns.push({ id: preset.id, btn })
      eqFlow.append(btn)
    }
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
  const headerIcon = new Gtk.Label({ label: "", cssClasses: ["ap-header-icon"] })
  const headerTitle = new Gtk.Label({ label: "AirPods", hexpand: true, xalign: 0, cssClasses: ["ap-header-title"] })
  const headerStatus = new Gtk.Label({ label: "", xalign: 0, cssClasses: ["ap-header-status"] })
  headerBox.append(headerIcon)
  const headerText = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL })
  headerText.append(headerTitle)
  headerText.append(headerStatus)
  headerBox.append(headerText)

  // ── Open App button ──
  const openAppBtn = new Gtk.Button({ cssClasses: ["ap-open-app"], label: "Open AirPods Helper" })
  openAppBtn.connect("clicked", () => {
    execAsync("airpods-app").catch(() => {
      execAsync(`bash -c '${GLib.get_home_dir()}/.local/bin/airpods-app &disown'`).catch(() => {})
    })
  })

  popupBox.append(headerBox)
  popupBox.append(batteryBox)
  popupBox.append(ancBox)
  popupBox.append(noiseSliderBox)
  popupBox.append(togglesBox)
  popupBox.append(eqBox)
  popupBox.append(earBox)
  popupBox.append(footerLabel)
  popupBox.append(openAppBtn)

  // ── Render function ──
  let updatingToggles = false

  function render() {
    const s = state
    const has = (f: string) => s.features.includes(f)
    const headphones = has("headphones")

    // Header
    headerTitle.label = s.modelName || s.model || "AirPods"
    headerStatus.label = has("anc") ? (ANC_LABELS[s.ancMode] ?? s.ancMode) : "Connected"

    // Battery — over-ear models report one battery (mirrored in Left/Right).
    clearBox(batteryBox)
    if (headphones) {
      batteryBox.append(makeBatRow("Battery", Math.max(s.batteryLeft, s.batteryRight), s.chargingLeft || s.chargingRight))
    } else {
      batteryBox.append(makeBatRow("Left", s.batteryLeft, s.chargingLeft))
      batteryBox.append(makeBatRow("Right", s.batteryRight, s.chargingRight))
      batteryBox.append(makeBatRow("Case", s.batteryCase, s.chargingCase))
    }

    // ANC buttons — only show if model supports ANC
    ancBox.visible = has("anc")
    if (has("anc")) {
      for (let i = 0; i < ancModes.length; i++) {
        // Hide adaptive button if model doesn't support it
        ancBtns[i].visible = ancModes[i].id !== "adaptive" || has("adaptive")
        const active = ancModes[i].id === s.ancMode
        ancBtns[i].cssClasses = active ? ["ap-anc-btn", "active"] : ["ap-anc-btn"]
      }
    }

    // Adaptive noise slider (don't fight the user while a change is pending)
    noiseSliderBox.visible = has("adaptive") && s.ancMode === "adaptive"
    if (noiseSliderBox.visible && sliderTimeout === null) {
      updatingSlider = true
      noiseSlider.set_value(s.adaptiveNoiseLevel)
      updatingSlider = false
    }

    // Toggles — only show relevant ones
    caRow.visible = has("ca")
    obRow.visible = has("one_bud_anc")
    micRow.visible = !headphones
    togglesBox.visible = caRow.visible || obRow.visible || micRow.visible

    updatingToggles = true
    if (has("ca")) caSwitch.active = s.conversationalAwareness
    if (has("one_bud_anc")) obSwitch.active = s.oneBudAnc
    updatingToggles = false

    for (let i = 0; i < micModes.length; i++) {
      const active = micModes[i].id === s.micMode
      micBtns[i].cssClasses = active ? ["ap-eq-btn", "ap-mic-btn", "active"] : ["ap-eq-btn", "ap-mic-btn"]
    }

    // EQ — EqPreset is the preset id ("" = off)
    rebuildEqButtons()
    for (const eq of eqBtns) {
      const active = eq.id === s.eqPreset
      eq.btn.cssClasses = active ? ["ap-eq-btn", "active"] : ["ap-eq-btn"]
      eq.btn.sensitive = s.eqStatus !== "unsupported" || eq.id === ""
    }
    eqStatusLabel.label = EQ_STATUS_LABELS[s.eqStatus] ?? s.eqStatus
    eqStatusLabel.cssClasses = ["ap-eq-status", `ap-eq-${s.eqStatus}`]
    let msg = ""
    if (s.eqStatus === "error" || s.eqStatus === "unsupported") msg = s.eqError || "EQ unavailable"
    else if (s.eqStatus === "waiting") msg = "Waiting for the AirPods audio output…"
    eqMsg.label = msg
    eqMsg.visible = msg !== ""
    eqMsg.cssClasses = ["ap-eq-msg", `ap-eq-${s.eqStatus}`]

    // Ears (no per-bud status on over-ear models)
    earBox.visible = !headphones
    earLeftLabel.label = `L: ${s.earLeft ? " In" : " Out"}`
    earRightLabel.label = `R: ${s.earRight ? " In" : " Out"}`

    // Footer
    footerLabel.label = s.firmware ? `${s.model}  ·  FW ${s.firmware}` : s.model
    footerLabel.visible = footerLabel.label !== ""
  }

  // Block toggle signals during programmatic updates
  caSwitch.connect("notify::active", () => { if (!updatingToggles) call("SetConversationalAwareness", new GLib.Variant("(b)", [caSwitch.active])) })
  obSwitch.connect("notify::active", () => { if (!updatingToggles) call("SetOneBudAnc", new GLib.Variant("(b)", [obSwitch.active])) })

  listeners.push(render)
  render()

  const popover = new Gtk.Popover()
  popover.set_child(popupBox)
  popover.connect("notify::visible", () => {
    if (popover.visible) { _lockBar(); sync(); loadPresets() }
    else _unlockBar()
  })

  const menuBtn = new Gtk.MenuButton({
    cssClasses: ["airpods-btn"],
    popover: popover,
    visible: state.connected,
  })
  menuBtn.set_child(barLabel)

  function lowestBattery(): number {
    const s = state
    if (s.features.includes("headphones")) return Math.max(s.batteryLeft, s.batteryRight)
    const known = [s.batteryLeft, s.batteryRight].filter((v) => v >= 0)
    return known.length ? Math.min(...known) : -1
  }

  listeners.push(() => {
    menuBtn.visible = state.connected
    const min = lowestBattery()
    barLabel.label = min >= 0 ? ` ${min}%` : ""
    if (min >= 0 && min <= 15) menuBtn.cssClasses = ["airpods-btn", "airpods-low"]
    else if (min >= 0 && min <= 30) menuBtn.cssClasses = ["airpods-btn", "airpods-warn"]
    else menuBtn.cssClasses = ["airpods-btn"]
  })
  // Apply the bar label for the initial state too.
  listeners[listeners.length - 1]()

  return menuBtn as Gtk.Widget
}
