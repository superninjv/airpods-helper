#!/usr/bin/env python3
"""DEV ONLY — a fake airpods-daemon implementing the org.costa.AirPods D-Bus
API v2 (docs/dbus-api.md) on the session bus, so the app/widget can be run and
tested without AirPods or Bluetooth.

    python3 app/dev/mock-daemon.py [--model pro2|max] [--disconnected]
                                   [--mic idle|streaming|unavailable|error]

Needs python3-gobject (gi). Stop the real daemon first, or run everything in
an isolated bus:  dbus-run-session -- sh -c 'python3 app/dev/mock-daemon.py & …'

Every few seconds the battery drifts so PropertiesChanged traffic is visible.
"""
import argparse
import re
import sys
import warnings

from gi.repository import Gio, GLib

# register_object(…callables…) is deprecated in newer PyGObject but still the
# simplest portable way to export an object.
warnings.filterwarnings("ignore", category=DeprecationWarning)

BUS = "org.costa.AirPods"
PATH = "/org/costa/AirPods"
IFACE = "org.costa.AirPods"

XML = f"""
<node>
  <interface name="{IFACE}">
    <property name="Connected" type="b" access="read"/>
    <property name="Address" type="s" access="read"/>
    <property name="Model" type="s" access="read"/>
    <property name="ModelName" type="s" access="read"/>
    <property name="Firmware" type="s" access="read"/>
    <property name="Features" type="as" access="read"/>
    <property name="BatteryLeft" type="i" access="read"/>
    <property name="BatteryRight" type="i" access="read"/>
    <property name="BatteryCase" type="i" access="read"/>
    <property name="ChargingLeft" type="b" access="read"/>
    <property name="ChargingRight" type="b" access="read"/>
    <property name="ChargingCase" type="b" access="read"/>
    <property name="EarLeft" type="b" access="read"/>
    <property name="EarRight" type="b" access="read"/>
    <property name="AncMode" type="s" access="read"/>
    <property name="AdaptiveNoiseLevel" type="y" access="read"/>
    <property name="ConversationalAwareness" type="b" access="read"/>
    <property name="ConversationalActivityState" type="s" access="read"/>
    <property name="OneBudAnc" type="b" access="read"/>
    <property name="VolumeSwipe" type="b" access="read"/>
    <property name="MicMode" type="s" access="read"/>
    <property name="Version" type="s" access="read"/>
    <property name="EqPreset" type="s" access="read"/>
    <property name="EqStatus" type="s" access="read"/>
    <property name="EqError" type="s" access="read"/>
    <property name="EqBackend" type="s" access="read"/>
    <property name="MicStatus" type="s" access="read"/>
    <property name="MicError" type="s" access="read"/>
    <property name="PauseOnRemoval" type="b" access="readwrite"/>
    <property name="ResumeOnInsert" type="b" access="readwrite"/>
    <property name="AutoReconnect" type="b" access="readwrite"/>
    <property name="PreferredDevice" type="s" access="readwrite"/>
    <property name="EqAutoLoad" type="b" access="readwrite"/>
    <property name="MicSource" type="b" access="readwrite"/>
    <method name="SetAncMode"><arg name="mode" type="s" direction="in"/></method>
    <method name="SetAdaptiveNoiseLevel"><arg name="level" type="y" direction="in"/></method>
    <method name="SetConversationalAwareness"><arg name="enabled" type="b" direction="in"/></method>
    <method name="SetOneBudAnc"><arg name="enabled" type="b" direction="in"/></method>
    <method name="SetVolumeSwipe"><arg name="enabled" type="b" direction="in"/></method>
    <method name="SetMicMode"><arg name="mode" type="s" direction="in"/></method>
    <method name="ListPaired"><arg type="a(ss)" direction="out"/></method>
    <method name="ConnectTo"><arg name="mac" type="s" direction="in"/></method>
    <method name="Disconnect"/>
    <method name="Pair"><arg name="mac" type="s" direction="in"/></method>
    <method name="QuickPairScan">
      <arg name="seconds" type="u" direction="in"/>
      <arg type="a(sssnb)" direction="out"/>
    </method>
    <method name="Reconnect"/>
    <method name="ListEqPresets"><arg type="as" direction="out"/></method>
    <method name="GetEqPresets"><arg type="a(sssb)" direction="out"/></method>
    <method name="GetEqPreset">
      <arg name="id" type="s" direction="in"/>
      <arg name="name" type="s" direction="out"/>
      <arg name="description" type="s" direction="out"/>
      <arg name="preamp" type="d" direction="out"/>
      <arg name="bands" type="a(sddd)" direction="out"/>
    </method>
    <method name="SetEqPreset"><arg name="id" type="s" direction="in"/></method>
    <method name="DisableEq"/>
    <method name="SaveEqPreset">
      <arg name="id" type="s" direction="in"/>
      <arg name="name" type="s" direction="in"/>
      <arg name="description" type="s" direction="in"/>
      <arg name="preamp" type="d" direction="in"/>
      <arg name="bands" type="a(sddd)" direction="in"/>
    </method>
    <method name="DeleteEqPreset"><arg name="id" type="s" direction="in"/></method>
    <signal name="DeviceConnected"><arg type="s"/></signal>
    <signal name="DeviceDisconnected"/>
    <signal name="EarDetectionChanged"><arg type="b"/><arg type="b"/></signal>
  </interface>
</node>
"""

TYPES = {}  # property name -> GVariant type string, filled from XML

PRESETS = {
    "flat": ("Flat", "No EQ applied", 0.0, [], False),
    "bass-boost": ("Bass Boost", "Enhanced low-end for bass-heavy music", -4.0, [
        ("lowshelf", 100.0, 0.7, 6.0), ("peaking", 250.0, 1.0, 2.0), ("peaking", 1000.0, 1.0, -1.0)], False),
    "vocal-clarity": ("Vocal Clarity", "Enhanced vocal presence and clarity", -2.0, [
        ("peaking", 200.0, 1.0, -2.0), ("peaking", 2500.0, 1.5, 3.5),
        ("peaking", 4000.0, 2.0, 2.0), ("highshelf", 8000.0, 0.7, 1.5)], False),
}

MODELS = {
    "pro2": dict(Model="A2931", ModelName="AirPods Pro 2 (USB-C)", Firmware="7A304",
                 Features=["anc", "adaptive", "ca", "one_bud_anc"]),
    "max": dict(Model="A2096", ModelName="AirPods Max", Firmware="6F8",
                Features=["anc", "headphones"], BatteryCase=-1),
}

PAIRED = [("AC:90:85:12:34:56", "Mock AirPods Pro"), ("F4:34:F0:AA:BB:CC", "Mock AirPods Max")]
# What the mic source reports while a session is up and MicSource is on,
# picked with --mic. The texts mirror the real daemon's MicError wording.
MIC_ERRORS = {
    "unavailable": "libfdk-aac is not installed (install libfdk-aac / libfdk-aac2 to use the AirPods microphone)",
    "error": "the AirPods stopped sending microphone audio",
}

MAC_RE = re.compile(r"^[0-9A-F]{2}(:[0-9A-F]{2}){5}$")


class Failed(Exception):
    name = "org.freedesktop.DBus.Error.Failed"


class InvalidArgs(Exception):
    name = "org.freedesktop.DBus.Error.InvalidArgs"


class Mock:
    def __init__(self, args):
        self.conn = None
        self.model = args.model
        self.mic = args.mic
        self.props = dict(
            Connected=False, Address="", Model="", ModelName="", Firmware="", Features=[],
            BatteryLeft=-1, BatteryRight=-1, BatteryCase=-1,
            ChargingLeft=False, ChargingRight=False, ChargingCase=False,
            EarLeft=False, EarRight=False, AncMode="off", AdaptiveNoiseLevel=50,
            ConversationalAwareness=False, ConversationalActivityState="normal",
            OneBudAnc=True, VolumeSwipe=True, MicMode="auto", Version="0.0.0-mock",
            EqPreset="bass-boost", EqStatus="waiting", EqError="", EqBackend="pipewire",
            PauseOnRemoval=True, ResumeOnInsert=True, AutoReconnect=True,
            PreferredDevice="", EqAutoLoad=True,
            MicSource=True, MicStatus="off", MicError="",
        )
        if not args.disconnected:
            self.props.update(self.connected_props(PAIRED[0][0]))

    def connected_props(self, mac):
        p = dict(Connected=True, Address=mac, BatteryLeft=80, BatteryRight=74, BatteryCase=40,
                 ChargingCase=True, EarLeft=True, EarRight=True, AncMode="noise")
        p.update(MODELS[self.model])
        if "headphones" in p["Features"]:
            p.update(BatteryRight=p["BatteryLeft"], EarLeft=False, EarRight=False)
        p["EqStatus"] = "active" if self.props.get("EqPreset") else "off"
        p.update(self.mic_props(True, self.props["MicSource"]))
        return p

    def mic_props(self, connected, enabled):
        # The real daemon only offers the source during an AAP session with the
        # setting on; otherwise MicStatus is "off".
        if not (connected and enabled):
            return dict(MicStatus="off", MicError="")
        return dict(MicStatus=self.mic, MicError=MIC_ERRORS.get(self.mic, ""))

    # ── helpers ──
    def set(self, **changes):
        changed = {k: v for k, v in changes.items() if self.props.get(k) != v}
        self.props.update(changed)
        if changed and self.conn:
            self.conn.emit_signal(None, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged",
                                  GLib.Variant("(sa{sv}as)", (IFACE, {k: self.variant(k) for k in changed}, [])))

    def variant(self, name):
        return GLib.Variant(TYPES[name], self.props[name])

    def need_connected(self):
        if not self.props["Connected"]:
            raise Failed("not connected")

    # ── methods ──
    def call(self, method, a):
        p = self.props
        if method == "SetAncMode":
            self.need_connected()
            if a[0] not in ("off", "noise", "transparency", "adaptive"):
                raise InvalidArgs(f"invalid mode {a[0]!r}")
            self.set(AncMode=a[0])
        elif method == "SetAdaptiveNoiseLevel":
            self.need_connected()
            self.set(AdaptiveNoiseLevel=min(100, a[0]))
        elif method == "SetConversationalAwareness":
            self.need_connected(); self.set(ConversationalAwareness=a[0])
        elif method == "SetOneBudAnc":
            self.need_connected(); self.set(OneBudAnc=a[0])
        elif method == "SetVolumeSwipe":
            self.need_connected(); self.set(VolumeSwipe=a[0])
        elif method == "SetMicMode":
            self.need_connected()
            if a[0] not in ("auto", "left", "right"):
                raise InvalidArgs("invalid mic mode")
            self.set(MicMode=a[0])
        elif method == "ListPaired":
            return GLib.Variant("(a(ss))", (PAIRED,))
        elif method == "ConnectTo":
            self.set(**self.connected_props(a[0]))
        elif method == "Disconnect":
            self.set(Connected=False, Address="", Model="", ModelName="", Firmware="", Features=[],
                     BatteryLeft=-1, BatteryRight=-1, BatteryCase=-1, EarLeft=False, EarRight=False,
                     EqStatus="waiting" if p["EqPreset"] else "off", **self.mic_props(False, p["MicSource"]))
        elif method == "Pair":
            raise Failed("pair: org.bluez.Error.AuthenticationFailed")
        elif method == "QuickPairScan":
            return GLib.Variant("(a(sssnb))", ([("58:D3:49:01:02:03", "AirPods Pro", "AirPods Pro 2 (USB-C)", -48, True)],))
        elif method == "Reconnect":
            pass
        elif method == "ListEqPresets":
            return GLib.Variant("(as)", (sorted(PRESETS),))
        elif method == "GetEqPresets":
            return GLib.Variant("(a(sssb))", ([(i, v[0], v[1], v[4]) for i, v in sorted(PRESETS.items())],))
        elif method == "GetEqPreset":
            if a[0] not in PRESETS:
                raise InvalidArgs(f"no such preset {a[0]!r}")
            n, d, pre, bands, _ = PRESETS[a[0]]
            return GLib.Variant("(ssda(sddd))", (n, d, pre, bands))
        elif method == "SetEqPreset":
            if a[0] not in PRESETS:
                raise InvalidArgs(f"no such preset {a[0]!r}")
            self.set(EqPreset=a[0], EqStatus="active" if p["Connected"] else "waiting")
        elif method == "DisableEq":
            self.set(EqPreset="", EqStatus="off")
        elif method == "SaveEqPreset":
            pid, name, desc, pre, bands = a
            if not re.fullmatch(r"[a-z0-9-]{1,48}", pid):
                raise InvalidArgs("invalid preset id")
            if len(bands) > 16 or not -24 <= pre <= 12:
                raise InvalidArgs("preset out of range")
            PRESETS[pid] = (name, desc, pre, [tuple(b) for b in bands], True)
        elif method == "DeleteEqPreset":
            if a[0] not in PRESETS or not PRESETS[a[0]][4]:
                raise InvalidArgs("only user presets can be deleted")
            del PRESETS[a[0]]
            if p["EqPreset"] == a[0]:
                self.set(EqPreset="", EqStatus="off")
        else:
            raise Failed(f"unknown method {method}")
        return None

    def set_prop(self, name, value):
        if name == "PreferredDevice" and value and not MAC_RE.match(value):
            raise InvalidArgs(f"invalid MAC {value!r}")
        self.set(**{name: value})
        if name == "MicSource":
            self.set(**self.mic_props(self.props["Connected"], value))

    def tick(self):
        if self.props["Connected"] and self.props["BatteryLeft"] > 5:
            lvl = self.props["BatteryLeft"] - 1
            upd = dict(BatteryLeft=lvl)
            if "headphones" in self.props["Features"]:
                upd["BatteryRight"] = lvl
            self.set(**upd)
        return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", choices=sorted(MODELS), default="pro2")
    ap.add_argument("--disconnected", action="store_true")
    ap.add_argument("--mic", choices=["idle", "starting", "streaming", "unavailable", "error"], default="idle",
                    help="MicStatus to report while connected with MicSource on")
    args = ap.parse_args()

    node = Gio.DBusNodeInfo.new_for_xml(XML)
    iface = node.interfaces[0]
    for prop in iface.properties:
        TYPES[prop.name] = prop.signature
    mock = Mock(args)

    def on_call(conn, sender, path, iname, method, params, inv):
        try:
            ret = mock.call(method, params.unpack())
            inv.return_value(ret)
        except (Failed, InvalidArgs) as e:
            inv.return_dbus_error(e.name, str(e))

    def on_get(conn, sender, path, iname, name):
        return mock.variant(name)

    def on_set(conn, sender, path, iname, name, value):
        try:
            mock.set_prop(name, value.unpack())
            return True
        except InvalidArgs as e:
            raise GLib.Error(str(e)) from e  # surfaces as a D-Bus error

    def on_bus(conn, name):
        mock.conn = conn
        conn.register_object(PATH, iface, on_call, on_get, on_set)

    def on_name(conn, name):
        print(f"mock daemon owns {name}", flush=True)

    def on_lost(conn, name):
        print(f"could not own {name} (is the real daemon running?)", file=sys.stderr)
        loop.quit()

    Gio.bus_own_name(Gio.BusType.SESSION, BUS, Gio.BusNameOwnerFlags.NONE, on_bus, on_name, on_lost)
    GLib.timeout_add_seconds(5, mock.tick)
    loop = GLib.MainLoop()
    loop.run()


if __name__ == "__main__":
    main()
