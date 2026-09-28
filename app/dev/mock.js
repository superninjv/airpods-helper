// DEV ONLY — fake `window.__TAURI__` so the frontend can be opened in a normal
// browser (no Tauri, no daemon, no AirPods). Not shipped: tauri.conf.json
// serves ../src only. Open app/dev/mock.html?scenario=<name>, where <name> is
// one of: pro2 (default), max, disconnected, absent, starting, editor,
// eq-waiting, eq-error. Every command is simulated in memory.
"use strict";

(function () {
  const params = new URLSearchParams(location.search);
  const scenario = params.get("scenario") || "pro2";

  const PRESETS = {
    flat: { name: "Flat", description: "No EQ applied", preamp: 0, bands: [], user_editable: false },
    "bass-boost": {
      name: "Bass Boost", description: "Enhanced low-end for bass-heavy music", preamp: -4, user_editable: false,
      bands: [
        { type: "lowshelf", freq: 100, q: 0.7, gain: 6 },
        { type: "peaking", freq: 250, q: 1, gain: 2 },
        { type: "peaking", freq: 1000, q: 1, gain: -1 },
      ],
    },
    "vocal-clarity": {
      name: "Vocal Clarity", description: "Enhanced vocal presence and clarity", preamp: -2, user_editable: false,
      bands: [
        { type: "peaking", freq: 200, q: 1, gain: -2 },
        { type: "peaking", freq: 2500, q: 1.5, gain: 3.5 },
        { type: "peaking", freq: 4000, q: 2, gain: 2 },
        { type: "highshelf", freq: 8000, q: 0.7, gain: 1.5 },
      ],
    },
    "airpods-pro-crinacle": {
      name: "AirPods Pro (Crinacle)", description: "Crinacle/AutoEQ Harman 2019v2 IEM target", preamp: -3.2, user_editable: false,
      bands: [
        { type: "peaking", freq: 25, q: 0.7, gain: 1.1 },
        { type: "peaking", freq: 501, q: 1.2, gain: -3.6 },
        { type: "peaking", freq: 860, q: 1, gain: 3.1 },
        { type: "peaking", freq: 1993, q: 2, gain: -2.4 },
        { type: "peaking", freq: 5600, q: 1.5, gain: 2.2 },
      ],
    },
    "late-night": {
      name: "Late Night", description: "Softer highs for long sessions", preamp: -1.5, user_editable: true,
      bands: [
        { type: "lowshelf", freq: 120, q: 0.7, gain: 2.5 },
        { type: "peaking", freq: 3200, q: 1.4, gain: -2.5 },
        { type: "highshelf", freq: 9000, q: 0.7, gain: -3 },
      ],
    },
  };

  const presetList = () =>
    Object.entries(PRESETS)
      .map(([id, p]) => ({ id, name: p.name, description: p.description, user_editable: p.user_editable }))
      .sort((a, b) => a.id.localeCompare(b.id));

  const base = {
    platform: "linux", daemon: "running", daemon_error: "", daemon_outdated: false, version: "0.3.0",
    connected: true, address: "AC:90:85:12:34:56", model: "A2931", model_name: "AirPods Pro 2 (USB-C)",
    firmware: "7A304", features: ["anc", "adaptive", "ca", "one_bud_anc"],
    battery_left: 82, battery_right: 76, battery_case: 41,
    charging_left: false, charging_right: false, charging_case: true,
    ear_left: true, ear_right: true,
    anc_mode: "adaptive", adaptive_noise_level: 60, conversational_awareness: true,
    conversational_activity_state: "normal", one_bud_anc: true, volume_swipe: true, mic_mode: "auto",
    eq_preset: "bass-boost", eq_status: "active", eq_error: "", eq_backend: "pipewire",
    eq_presets: presetList(),
    pause_on_removal: true, resume_on_insert: true, auto_reconnect: true,
    preferred_device: "", eq_auto_load: true, start_on_login: false,
  };

  const SCENARIOS = {
    pro2: {},
    editor: {},
    "eq-waiting": { eq_status: "waiting", eq_preset: "vocal-clarity" },
    "eq-error": { eq_status: "error", eq_error: "filter-chain module failed to load: libpipewire-module-filter-chain not found" },
    max: {
      model: "A2096", model_name: "AirPods Max", firmware: "6F8", features: ["anc", "headphones"],
      battery_left: 64, battery_right: 64, battery_case: -1, charging_left: true, charging_right: true,
      charging_case: false, ear_left: false, ear_right: false, anc_mode: "noise",
      eq_preset: "", eq_status: "off",
    },
    disconnected: {
      connected: false, address: "", model: "", model_name: "", firmware: "", features: [],
      battery_left: -1, battery_right: -1, battery_case: -1, charging_case: false,
      ear_left: false, ear_right: false, anc_mode: "off", eq_status: "waiting", eq_preset: "late-night",
      preferred_device: "AC:90:85:12:34:56",
    },
    absent: {
      daemon: "absent", connected: false,
      daemon_error: "The AirPods daemon failed to start: Process org.costa.AirPods exited with status 1",
      features: [], eq_presets: [], version: "", eq_backend: "none", eq_status: "off", eq_preset: "",
      model_name: "", model: "", firmware: "",
    },
    starting: { daemon: "starting", connected: false, features: [], eq_presets: [], version: "" },
  };

  const state = Object.assign({}, base, SCENARIOS[scenario] || {});
  const listeners = {};

  function emit(event, payload) {
    for (const cb of listeners[event] || []) cb({ event, payload: JSON.parse(JSON.stringify(payload)) });
  }
  const push = () => emit("status", state);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  const PAIRED = [
    { address: "AC:90:85:12:34:56", name: "Kevin's AirPods Pro" },
    { address: "F4:34:F0:AA:BB:CC", name: "AirPods Max" },
  ];

  async function invoke(cmd, args = {}) {
    await sleep(60);
    const needConn = () => {
      if (!state.connected) throw "Not connected";
    };
    switch (cmd) {
      case "get_status": return JSON.parse(JSON.stringify(state));
      case "retry_daemon": await sleep(600); throw "The AirPods daemon is not running.";
      case "set_anc_mode": needConn(); state.anc_mode = args.mode; break;
      case "set_adaptive_noise_level": needConn(); state.adaptive_noise_level = args.level; break;
      case "set_conversational_awareness": needConn(); state.conversational_awareness = args.enabled; break;
      case "set_one_bud_anc": needConn(); state.one_bud_anc = args.enabled; break;
      case "set_volume_swipe": needConn(); state.volume_swipe = args.enabled; break;
      case "set_mic_mode": needConn(); state.mic_mode = args.mode; break;
      case "set_setting":
        if (args.key === "preferred_device" && args.value && !/^[0-9A-F]{2}(:[0-9A-F]{2}){5}$/.test(args.value)) {
          throw "Invalid MAC address";
        }
        state[args.key] = args.value;
        break;
      case "set_start_on_login": state.start_on_login = args.enabled; break;
      case "list_paired": await sleep(200); return PAIRED;
      case "connect_device": await sleep(1200); throw "BlueZ connect: br-connection-page-timeout";
      case "disconnect_device": Object.assign(state, SCENARIOS.disconnected); break;
      case "reconnect": break;
      case "pair_device": await sleep(1500); throw "pair: org.bluez.Error.AuthenticationFailed";
      case "quick_pair_scan":
        await sleep(args.seconds * 100);
        return [{ address: "58:D3:49:01:02:03", name: "AirPods Pro", model: "AirPods Pro 2 (USB-C)", rssi: -48, in_pair_mode: true }];
      case "refresh_eq_presets": state.eq_presets = presetList(); push(); return state.eq_presets;
      case "get_eq_preset": {
        const p = PRESETS[args.id];
        if (!p) throw `no such preset '${args.id}'`;
        return { id: args.id, name: p.name, description: p.description, preamp: p.preamp, bands: p.bands };
      }
      case "set_eq_preset":
        state.eq_preset = args.id;
        state.eq_status = !args.id ? "off" : state.connected ? "active" : "waiting";
        break;
      case "save_eq_preset": {
        const p = args.preset;
        PRESETS[p.id] = { ...p, user_editable: true };
        state.eq_presets = presetList();
        break;
      }
      case "delete_eq_preset":
        delete PRESETS[args.id];
        state.eq_presets = presetList();
        if (state.eq_preset === args.id) Object.assign(state, { eq_preset: "", eq_status: "off" });
        break;
      default:
        throw `mock: unknown command ${cmd}`;
    }
    setTimeout(push, 40);
    return null;
  }

  window.__TAURI__ = {
    core: { invoke },
    event: {
      listen(event, cb) {
        (listeners[event] = listeners[event] || []).push(cb);
        return Promise.resolve(() => {
          listeners[event] = listeners[event].filter((f) => f !== cb);
        });
      },
    },
  };

  // Scenario set-up that needs the UI.
  window.addEventListener("DOMContentLoaded", () => {
    setTimeout(() => {
      if (scenario === "editor") {
        // Customize the active built-in preset and tweak a band.
        document.getElementById("eq-edit").click();
        const gain = document.querySelector('#ed-bands .band:nth-child(2) [data-key="gain"]');
        if (gain) {
          gain.value = "4.5";
          gain.dispatchEvent(new Event("input"));
        }
        document.getElementById("ed-add").click();
      }
      if (params.get("settings") === "open") document.getElementById("settings-card").open = true;
      if (params.get("toast")) {
        // Exercise the error toast path with a realistic daemon error.
        emit("backend-error", "Not connected");
      }
    }, 500);
  });
})();
