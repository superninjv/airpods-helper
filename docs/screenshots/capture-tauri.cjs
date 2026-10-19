// Capture the real frontend with demo D-Bus responses; never loads a native bridge.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");
const { pathToFileURL } = require("node:url");

const source = path.resolve(__dirname, "../../app/src");
const output = path.resolve(process.argv[2] || __dirname);
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "airpods-screenshots-"));
let iconPixels;

function trayPreview(iconPixels) {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 64;
  canvas.style = "position:fixed;left:8px;top:8px;width:64px;height:64px";
  canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(iconPixels), 64, 64), 0, 0);
  document.body.append(canvas);
}

function bridge(page) {
  const preset = {
    id: "bass", name: "Bass Boost", description: "Bass preset", preamp: -3,
    bands: [{ type: "peaking", freq: 100, q: 1, gain: 6 }],
  };
  const state = {
    devices_revision: 0, daemon: "running", daemon_error: "", daemon_outdated: false, version: "0.3.0",
    connected: true, address: "AA:BB:CC:DD:EE:FF", model: "A2698", model_name: "AirPods Pro 2", firmware: "7B21",
    features: ["anc", "adaptive", "ca", "one_bud_anc"],
    battery_left: 82, battery_right: 76, battery_case: 43, charging_left: false, charging_right: false,
    charging_case: true, ear_left: true, ear_right: true, anc_mode: "noise", adaptive_noise_level: 50,
    conversational_awareness: false, conversational_activity_state: "normal", one_bud_anc: false,
    volume_swipe: true, mic_mode: "auto", eq_preset: "bass", eq_status: "active", eq_error: "", eq_backend: "pipewire",
    eq_presets: [
      { id: "airpods-pro-crinacle", name: "AirPods Pro (Crinacle)" },
      { id: "bass", name: "Bass Boost" }, { id: "flat", name: "Flat" }, { id: "vocal", name: "Vocal Clarity" },
    ],
    mic_status: "idle", mic_error: "", pause_on_removal: true, resume_on_insert: true, auto_reconnect: true,
    preferred_device: "AA:BB:CC:DD:EE:FF", eq_auto_load: true, mic_source: true, start_on_login: false,
  };
  window.__TAURI__ = {
    core: { invoke: async command => {
      if (command === "get_status") return state;
      if (command === "list_paired") return [{ address: state.address, name: "Alex’s AirPods" }];
      if (command === "get_eq_preset") return preset;
      if (command === "is_appimage") return false;
      throw new Error(`Unexpected screenshot command: ${command}`);
    } },
    event: { listen: async () => () => {} },
    app: { getVersion: async () => state.version },
    window: { getCurrentWindow: () => ({ startResizeDragging: async () => {} }) },
  };
  const ready = new MutationObserver(() => {
    if (document.getElementById("app").getAttribute("aria-busy") !== "false") return;
    ready.disconnect();
    document.getElementById(`tab-${page === "equalizer" ? "controls" : page}`).click();
    if (page === "devices") document.querySelector('#device-list [aria-label^="Rename "]').click();
    if (page === "equalizer") {
      document.getElementById("eq-edit").click();
      const content = document.querySelector(".content");
      content.scrollTop += document.getElementById("eq-card").getBoundingClientRect().top - content.getBoundingClientRect().top;
    }
  });
  ready.observe(document.getElementById("app"), { attributes: true, attributeFilter: ["aria-busy"] });
}

async function capture(page) {
  const width = page === "tray" ? 80 : 480;
  const height = page === "tray" ? 80 : page === "equalizer" ? 980 : page === "devices" ? 920 : 820;
  const profile = path.join(temporary, `profile-${page}`);
  fs.mkdirSync(profile);
  fs.writeFileSync(path.join(profile, "user.js"), 'user_pref("ui.systemUsesDarkTheme", 1);\n');
  const html = page === "tray"
    ? `<!doctype html><html><head><link rel="stylesheet" href="${pathToFileURL(path.join(source, "style.css")).href}"></head><body><script>(${trayPreview.toString()})(${JSON.stringify(iconPixels)});</script></body></html>`
    : fs.readFileSync(path.join(source, "index.html"), "utf8")
    .replace("<head>", `<head><base href="${pathToFileURL(source + path.sep).href}">`)
    .replace('<script src="eq.js">', `<script>(${bridge.toString()})(${JSON.stringify(page)});</script><script src="eq.js">`);
  const input = path.join(temporary, `${page}.html`);
  const target = path.join(output, `tauri-${page}.png`);
  fs.writeFileSync(input, html);
  fs.rmSync(target, { force: true });
  const log = fs.openSync(path.join(temporary, `${page}.log`), "w");
  const browser = spawn("firefox", ["--headless", "--no-remote", "--profile", profile,
    `--window-size=${width},${height}`, "--screenshot", target, pathToFileURL(input).href], {
    env: { ...process.env, MOZ_HEADLESS: "1", MOZ_CRASHREPORTER_DISABLE: "1", GSETTINGS_BACKEND: "memory" },
    stdio: ["ignore", log, log],
  });
  fs.closeSync(log);
  let startError;
  browser.on("error", error => { startError = error; });
  try {
    for (let attempt = 0; attempt < 100 && !fs.existsSync(target) && !startError && browser.exitCode === null; attempt++)
      await new Promise(resolve => setTimeout(resolve, 200));
    if (startError) throw startError;
    assert.ok(fs.existsSync(target), `No screenshot: inspect ${temporary}/${page}.log`);
    const png = fs.readFileSync(target);
    assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
    assert.equal(png.readUInt32BE(16), width);
    assert.equal(png.readUInt32BE(20), height);
    process.stdout.write(`${target}\n`);
  } finally {
    browser.kill("SIGTERM");
    await new Promise(resolve => {
      if (startError || browser.exitCode !== null || browser.signalCode !== null) return resolve();
      const timer = setTimeout(() => browser.kill("SIGKILL"), 2000);
      browser.once("exit", () => { clearTimeout(timer); resolve(); });
    });
  }
}

(async () => {
  fs.mkdirSync(output, { recursive: true });
  const rendered = spawnSync("cargo", ["test", "--manifest-path", path.join(source, "../src-tauri/Cargo.toml"),
    "--release", "--locked", "tray_icon::tests::export_screenshot", "--", "--ignored"], {
    env: { ...process.env, AIRPODS_SCREENSHOTS: temporary }, stdio: "inherit",
  });
  if (rendered.error) throw rendered.error;
  assert.equal(rendered.status, 0, "Could not render the Tauri tray icon");
  iconPixels = Array.from(fs.readFileSync(path.join(temporary, "tauri-tray.rgba")));
  assert.equal(iconPixels.length, 64 * 64 * 4);
  for (const page of ["controls", "equalizer", "devices", "settings", "tray"]) await capture(page);
  fs.rmSync(temporary, { recursive: true, force: true });
})().catch(error => { console.error(error); process.exitCode = 1; });
