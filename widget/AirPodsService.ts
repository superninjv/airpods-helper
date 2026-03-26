import Gio from "gi://Gio"
import GLib from "gi://GLib"
import { createState } from "gnim"

// D-Bus proxy for org.costa.AirPods
// Polls properties via standard D-Bus Properties interface
// Listens for PropertiesChanged signals for reactive updates

const BUS_NAME = "org.costa.AirPods"
const OBJECT_PATH = "/org/costa/AirPods"
const IFACE_NAME = "org.costa.AirPods"

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
  eqPreset: string
  model: string
  firmware: string
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
  eqPreset: "",
  model: "",
  firmware: "",
}

const [getState, setState] = createState<AirPodsState>({ ...DEFAULT_STATE })
export { getState }

let proxy: Gio.DBusProxy | null = null

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
    eqPreset: getProperty("EqPreset") ?? "",
    model: getProperty("Model") ?? "",
    firmware: getProperty("Firmware") ?? "",
  }
}

function syncState() {
  if (!proxy) {
    setState({ ...DEFAULT_STATE })
    return
  }
  const props = readAllProperties()
  setState({ ...DEFAULT_STATE, available: true, ...props })
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
      (_proxy: Gio.DBusProxy, changed: GLib.Variant, _invalidated: string[]) => {
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

// D-Bus method calls
export async function setAncMode(mode: string) {
  if (!proxy) return
  proxy.call(
    "SetAncMode",
    new GLib.Variant("(s)", [mode]),
    Gio.DBusCallFlags.NONE,
    5000,
    null,
    null,
  )
}

export async function setConversationalAwareness(enabled: boolean) {
  if (!proxy) return
  proxy.call(
    "SetConversationalAwareness",
    new GLib.Variant("(b)", [enabled]),
    Gio.DBusCallFlags.NONE,
    5000,
    null,
    null,
  )
}

export async function setOneBudAnc(enabled: boolean) {
  if (!proxy) return
  proxy.call(
    "SetOneBudAnc",
    new GLib.Variant("(b)", [enabled]),
    Gio.DBusCallFlags.NONE,
    5000,
    null,
    null,
  )
}

export async function setAdaptiveNoiseLevel(level: number) {
  if (!proxy) return
  proxy.call(
    "SetAdaptiveNoiseLevel",
    new GLib.Variant("(y)", [Math.min(100, Math.max(0, level))]),
    Gio.DBusCallFlags.NONE,
    5000,
    null,
    null,
  )
}

export async function setEqPreset(name: string) {
  if (!proxy) return
  proxy.call(
    "SetEqPreset",
    new GLib.Variant("(s)", [name]),
    Gio.DBusCallFlags.NONE,
    5000,
    null,
    null,
  )
}

export async function disableEq() {
  if (!proxy) return
  proxy.call("DisableEq", null, Gio.DBusCallFlags.NONE, 5000, null, null)
}

export async function reconnect() {
  if (!proxy) return
  proxy.call("Reconnect", null, Gio.DBusCallFlags.NONE, 5000, null, null)
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
