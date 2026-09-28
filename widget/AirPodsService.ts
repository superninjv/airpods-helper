import Gio from "gi://Gio"
import GLib from "gi://GLib"
import { createState } from "gnim"

// D-Bus proxy for org.costa.AirPods (API v2, see docs/dbus-api.md)
// Reads cached properties from the proxy and listens for PropertiesChanged
// signals for reactive updates.

const BUS_NAME = "org.costa.AirPods"
const OBJECT_PATH = "/org/costa/AirPods"
const IFACE_NAME = "org.costa.AirPods"

export interface EqPresetInfo {
  id: string
  name: string
  description: string
  userEditable: boolean
}

export interface AirPodsState {
  available: boolean
  connected: boolean
  batteryLeft: number
  batteryRight: number
  batteryCase: number
  chargingLeft: boolean
  chargingRight: boolean
  chargingCase: boolean
  ancMode: string
  earLeft: boolean
  earRight: boolean
  conversationalAwareness: boolean
  adaptiveNoiseLevel: number
  oneBudAnc: boolean
  volumeSwipe: boolean
  micMode: string
  /** Preset id ("" = EQ off). */
  eqPreset: string
  /** off | active | waiting | error | unsupported */
  eqStatus: string
  eqError: string
  eqPresets: EqPresetInfo[]
  model: string
  modelName: string
  firmware: string
  features: string[]
}

const DEFAULT_STATE: AirPodsState = {
  available: false,
  connected: false,
  batteryLeft: -1,
  batteryRight: -1,
  batteryCase: -1,
  chargingLeft: false,
  chargingRight: false,
  chargingCase: false,
  ancMode: "off",
  earLeft: false,
  earRight: false,
  conversationalAwareness: false,
  adaptiveNoiseLevel: 50,
  oneBudAnc: true,
  volumeSwipe: true,
  micMode: "auto",
  eqPreset: "",
  eqStatus: "off",
  eqError: "",
  eqPresets: [],
  model: "",
  modelName: "",
  firmware: "",
  features: [],
}

const [getState, setState] = createState<AirPodsState>({ ...DEFAULT_STATE })
export { getState }

/** True for over-ear models (single battery, no case, no per-bud ear status). */
export function isHeadphones(s: AirPodsState): boolean {
  return s.features.includes("headphones")
}

let proxy: Gio.DBusProxy | null = null
let eqPresets: EqPresetInfo[] = []

function unpackVariant(v: GLib.Variant): any {
  if (!v) return null
  return v.deepUnpack()
}

function getProperty(name: string): any {
  if (!proxy) return null
  const v = proxy.get_cached_property(name)
  return v ? unpackVariant(v) : null
}

function readAllProperties(): Partial<AirPodsState> {
  return {
    connected: getProperty("Connected") ?? false,
    batteryLeft: getProperty("BatteryLeft") ?? -1,
    batteryRight: getProperty("BatteryRight") ?? -1,
    batteryCase: getProperty("BatteryCase") ?? -1,
    chargingLeft: getProperty("ChargingLeft") ?? false,
    chargingRight: getProperty("ChargingRight") ?? false,
    chargingCase: getProperty("ChargingCase") ?? false,
    ancMode: getProperty("AncMode") ?? "off",
    earLeft: getProperty("EarLeft") ?? false,
    earRight: getProperty("EarRight") ?? false,
    conversationalAwareness: getProperty("ConversationalAwareness") ?? false,
    adaptiveNoiseLevel: getProperty("AdaptiveNoiseLevel") ?? 50,
    oneBudAnc: getProperty("OneBudAnc") ?? true,
    volumeSwipe: getProperty("VolumeSwipe") ?? true,
    micMode: getProperty("MicMode") ?? "auto",
    eqPreset: getProperty("EqPreset") ?? "",
    eqStatus: getProperty("EqStatus") ?? "off",
    eqError: getProperty("EqError") ?? "",
    model: getProperty("Model") ?? "",
    modelName: getProperty("ModelName") ?? "",
    firmware: getProperty("Firmware") ?? "",
    features: getProperty("Features") ?? [],
  }
}

function syncState() {
  if (!proxy) {
    setState({ ...DEFAULT_STATE })
    return
  }
  const props = readAllProperties()
  setState({ ...DEFAULT_STATE, available: true, ...props, eqPresets })
  if (props.eqPreset && !eqPresets.some((p) => p.id === props.eqPreset)) refreshEqPresets()
}

let presetsLoading = false

/** Reload the preset list via GetEqPresets (ListEqPresets on v1 daemons). */
export function refreshEqPresets() {
  if (!proxy || presetsLoading) return
  presetsLoading = true
  const done = (list: EqPresetInfo[]) => {
    presetsLoading = false
    eqPresets = list
    setState({ ...getState(), eqPresets })
  }
  proxy.call("GetEqPresets", null, Gio.DBusCallFlags.NONE, 5000, null, (p, res) => {
    try {
      const [list] = p!.call_finish(res).deepUnpack() as [[string, string, string, boolean][]]
      done(list.map(([id, name, description, userEditable]) => ({ id, name, description, userEditable })))
    } catch {
      p!.call("ListEqPresets", null, Gio.DBusCallFlags.NONE, 5000, null, (p2, res2) => {
        try {
          const [ids] = p2!.call_finish(res2).deepUnpack() as [string[]]
          done(ids.map((id) => ({ id, name: id, description: "", userEditable: false })))
        } catch (e) {
          presetsLoading = false
          console.error(`airpods: listing EQ presets failed: ${e}`)
        }
      })
    }
  })
}

// Signal listeners
const signalHandlers: number[] = []

function connectProxy() {
  if (proxy) {
    signalHandlers.forEach((id) => proxy!.disconnect(id))
    signalHandlers.length = 0
  }

  try {
    proxy = Gio.DBusProxy.new_for_bus_sync(
      Gio.BusType.SESSION,
      Gio.DBusProxyFlags.NONE,
      null,
      BUS_NAME,
      OBJECT_PATH,
      IFACE_NAME,
      null,
    )

    // Listen for property changes
    const propId = proxy.connect(
      "g-properties-changed",
      (_proxy: Gio.DBusProxy, _changed: GLib.Variant, _invalidated: string[]) => {
        syncState()
      },
    )
    signalHandlers.push(propId)

    // Listen for custom signals
    const sigId = proxy.connect(
      "g-signal",
      (_proxy: Gio.DBusProxy, _sender: string | null, signalName: string, params: GLib.Variant) => {
        if (signalName === "DeviceConnected") {
          const unpacked = params.deepUnpack<unknown[]>()
          const model = Array.isArray(unpacked) && typeof unpacked[0] === "string" ? unpacked[0] : ""
          onDeviceConnected(model)
        } else if (signalName === "DeviceDisconnected") {
          onDeviceDisconnected()
        }
      },
    )
    signalHandlers.push(sigId)

    syncState()
    refreshEqPresets()
  } catch (e) {
    proxy = null
    setState({ ...DEFAULT_STATE })
  }
}

// Connection event callbacks (set by popup)
let onDeviceConnected: (model: string) => void = () => {}
let onDeviceDisconnected: () => void = () => {}

export function setConnectionCallbacks(
  onConnect: (model: string) => void,
  onDisconnect: () => void,
) {
  onDeviceConnected = onConnect
  onDeviceDisconnected = onDisconnect
}

// D-Bus method calls (async; failures are logged, state follows PropertiesChanged)
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

export async function setAncMode(mode: string) {
  call("SetAncMode", new GLib.Variant("(s)", [mode]))
}

export async function setConversationalAwareness(enabled: boolean) {
  call("SetConversationalAwareness", new GLib.Variant("(b)", [enabled]))
}

export async function setOneBudAnc(enabled: boolean) {
  call("SetOneBudAnc", new GLib.Variant("(b)", [enabled]))
}

export async function setVolumeSwipe(enabled: boolean) {
  call("SetVolumeSwipe", new GLib.Variant("(b)", [enabled]))
}

/** "auto" | "left" | "right" */
export async function setMicMode(mode: string) {
  call("SetMicMode", new GLib.Variant("(s)", [mode]))
}

export async function setAdaptiveNoiseLevel(level: number) {
  call("SetAdaptiveNoiseLevel", new GLib.Variant("(y)", [Math.min(100, Math.max(0, Math.round(level)))]))
}

/** Select a preset by id (see `eqPresets`); "" turns EQ off. */
export async function setEqPreset(id: string) {
  if (id === "") call("DisableEq")
  else call("SetEqPreset", new GLib.Variant("(s)", [id]))
}

export async function disableEq() {
  call("DisableEq")
}

export async function reconnect() {
  call("Reconnect")
}

// Watch for daemon appearing/disappearing on the bus
function watchBus() {
  Gio.bus_watch_name(
    Gio.BusType.SESSION,
    BUS_NAME,
    Gio.BusNameWatcherFlags.NONE,
    () => {
      // Name appeared
      connectProxy()
    },
    () => {
      // Name vanished
      proxy = null
      setState({ ...DEFAULT_STATE })
    },
  )
}

// Initialize
watchBus()
connectProxy()
