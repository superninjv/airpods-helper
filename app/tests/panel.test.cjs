const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const Panel = require("../src/panel.js");
const source = name => fs.readFileSync(path.join(__dirname, "../src", name), "utf8");
const context = vm.createContext({});
vm.runInContext(source("eq.js"), context);
const Eq = context.AirPodsEq;

test("header resolves the personal device name by Bluetooth address", () => {
  const devices = [{ address: "AA:BB:CC:DD:EE:FF", name: "Nico’s AirPods" }, { address: "11:22:33:44:55:66", name: "Other AirPods" }];
  assert.equal(Panel.deviceName("aa:bb:cc:dd:ee:ff", devices), "Nico’s AirPods");
  assert.equal(Panel.deviceName("11:22:33:44:55:66", devices), "Other AirPods");
  assert.equal(Panel.deviceName("00:00:00:00:00:00", devices), "AirPods");
  assert.equal(Panel.deviceName("", devices), "AirPods");
  assert.equal(Panel.deviceName(undefined, []), "AirPods");
});

test("Devices contains preferred device and Settings contains About", () => {
  const html = source("index.html");
  const devices = html.slice(html.indexOf('id="page-devices"'), html.indexOf('id="page-settings"'));
  const settings = html.slice(html.indexOf('id="page-settings"'));
  for (const id of ["devices-card", "paired-card", "nearby-card", "pair-card", "preferred-card", "preferred-form", "s-preferred"])
    assert.ok(devices.includes(`id="${id}"`), id);
  assert.ok(!settings.includes('id="preferred-form"'));
  assert.ok(settings.includes('id="about-card"'));
  assert.ok(settings.includes('id="about-details"'));
  assert.ok(settings.includes('id="versions" class="about-details"'));
  assert.ok(settings.includes('id="btn-releases"'));
  assert.ok(!html.includes('id="btn-error-preview"'));
  assert.ok(!html.includes("MAC address"));
  const capabilities = JSON.parse(fs.readFileSync(path.join(__dirname, "../src-tauri/capabilities/default.json")));
  assert.ok(capabilities.permissions.includes("core:app:allow-version"));
  assert.ok(capabilities.permissions.includes("allow-rename-device"));
  assert.ok(devices.includes('id="rename-form"'));
  assert.ok(devices.includes('id="preferred-name"'));
  assert.ok(devices.includes('id="preferred-address"'));
});

test("tabs select one page and support keyboard navigation", () => {
  class Tab extends EventTarget {
    constructor(name) { super(); this.dataset = { page: name }; this.attributes = {}; }
    setAttribute(name, value) { this.attributes[name] = value; }
    focus() { this.focused = true; }
  }
  const names = ["controls", "devices", "settings"];
  const tabs = names.map(name => new Tab(name));
  const pages = names.map(name => ({ dataset: { page: name } }));
  let selected;
  Panel.wireTabs(tabs, pages, name => selected = name);
  function check(index) {
    assert.equal(selected, names[index]);
    tabs.forEach((tab, i) => {
      assert.equal(tab.attributes["aria-selected"], String(i === index));
      assert.equal(tab.tabIndex, i === index ? 0 : -1);
      assert.equal(pages[i].hidden, i !== index);
    });
  }
  function key(index, value) {
    const event = new Event("keydown", { cancelable: true });
    event.key = value;
    tabs[index].dispatchEvent(event);
    assert.equal(event.defaultPrevented, true);
  }
  check(0);
  tabs[1].dispatchEvent(new Event("click"));
  check(1);
  key(1, "End"); check(2);
  key(2, "ArrowRight"); check(0);
  key(0, "ArrowLeft"); check(2);
  key(2, "Home"); check(0);
  assert.equal(tabs[0].focused, true);
});

test("only the redundant built-in Flat preset is hidden", () => {
  const flat = { id: "flat", preamp: 0, bands: [] };
  const crinacle = { id: "airpods-pro-crinacle", name: "AirPods Pro (Crinacle)" };
  const presets = [flat, crinacle];
  assert.equal(Eq.visiblePresets(presets).length, 1);
  assert.equal(Eq.selectedPresetId("flat", presets), "");
  assert.equal(Eq.presetLabel(crinacle), "Crinacle");
  assert.deepEqual(Eq.response(flat), Eq.response(null));
  const custom = { ...flat, user_editable: true };
  assert.equal(Eq.visiblePresets([custom]).length, 1);
  assert.equal(Eq.selectedPresetId("flat", [custom]), "flat");
  assert.equal(Eq.presetLabel({ ...crinacle, user_editable: true }), crinacle.name);
});

test("frameless edges request the native resize direction", () => {
  const directions = ["North", "South", "East", "West", "NorthEast", "NorthWest", "SouthEast", "SouthWest"];
  const handles = directions.map(resize => Object.assign(new EventTarget(), { dataset: { resize } }));
  const requested = [];
  Panel.wireResize(handles, direction => requested.push(direction));
  for (const handle of handles) {
    for (const button of [2, 0]) {
      const event = new Event("mousedown", { cancelable: true });
      event.button = button;
      handle.dispatchEvent(event);
      assert.equal(event.defaultPrevented, button === 0);
    }
  }
  assert.deepEqual(requested, directions);
  const capabilities = JSON.parse(fs.readFileSync(path.join(__dirname, "../src-tauri/capabilities/default.json")));
  assert.ok(capabilities.permissions.includes("core:window:allow-start-resize-dragging"));
});

test("EQ redraws to its displayed size with borders and frequency units", () => {
  class Element {
    constructor() { this.attributes = {}; this.children = []; }
    setAttribute(name, value) { this.attributes[name] = value; }
    append(child) { this.children.push(child); }
    replaceChildren() { this.children = []; }
  }
  context.document = { createElementNS: () => new Element() };
  const svg = new Element();
  svg.clientHeight = 200;
  for (const width of [240, 360, 1200]) {
    svg.clientWidth = width;
    Eq.plot(svg, { bands: [{ type: "peaking", freq: 20000, gain: 5, q: 1 }], preamp: 0 });
    assert.equal(svg.attributes.viewBox, `0 0 ${width} 200`);
    const frame = svg.children[0].children[0].attributes;
    assert.ok(+frame.x > 0 && +frame.x + +frame.width < width);
    const labels = svg.children[1].children;
    assert.deepEqual(Array.from(labels.slice(0, 5), label => label.textContent), ["20 Hz", "100 Hz", "1 kHz", "10 kHz", "20 kHz"]);
    assert.ok(labels.slice(5).every(label => label.textContent.endsWith(" dB")));
    assert.ok(labels.slice(0, 5).every(label => label.attributes.transform.endsWith("rotate(60)")));
    const dots = svg.children.at(-1).children;
    for (const dot of dots) assert.ok(+dot.attributes.cx + +dot.attributes.r < width);
  }
  for (const [gain, count, expectedRange] of [[0, 1, 12], [13, 1, 18], [15, 2, 30], [-21, 2, 42]]) {
    Eq.plot(svg, { bands: Array.from({ length: count }, () => ({ type: "peaking", freq: 1000, gain, q: 1 })), preamp: -6 });
    assert.deepEqual(Array.from(svg.children[1].children.slice(5), label => label.textContent), [`${expectedRange} dB`, "0 dB", `-${expectedRange} dB`]);
  }
});

test("battery tiles show only the In ear badge and retain accessible ear status", () => {
  const js = source("main.js");
  const batteryContext = vm.createContext({
    h: (tag, attributes, ...children) => ({ tag, ...attributes, children, style: {}, append(child) { this.children.push(child); } }),
    icon: id => ({ icon: id }),
  });
  vm.runInContext(js.slice(js.indexOf("  function batteryTile("), js.indexOf("  function renderBattery(")), batteryContext);
  for (const ear of [true, false, undefined]) {
    const tile = batteryContext.batteryTile("Left", 50, false, ear);
    const badges = tile.children[0].children.filter(child => child.class === "ear-chip in");
    assert.equal(badges.length, ear === true ? 1 : 0);
    if (ear === true) assert.equal(badges[0].text, "In ear");
    if (ear === false) assert.ok(tile["aria-label"].includes("not in ear"));
  }
});

test("error toasts wait for dismissal while informational toasts expire", () => {
  const timers = [];
  const box = { children: [], append(node) { this.children.push(node); } };
  const toastContext = vm.createContext({
    $: () => box,
    h: (tag, attributes, ...children) => Object.assign(new EventTarget(), attributes, {
      children, classList: { add() {} }, remove() { this.removed = true; },
    }),
    icon: () => ({}),
    setTimeout: (callback, delay) => timers.push({ callback, delay }),
  });
  const js = source("main.js");
  vm.runInContext(js.slice(js.indexOf("  function toast("), js.indexOf("  /** invoke()")), toastContext);
  toastContext.toast("Connection failed");
  const error = box.children[0];
  assert.equal(error.role, "alert");
  assert.equal(timers.length, 0);
  toastContext.toast("Saved", "info");
  assert.equal(timers.length, 1);
  assert.equal(timers[0].delay, 3500);
  error.children[1].dispatchEvent(new Event("click"));
  assert.equal(timers[1].delay, 180);
  timers[1].callback();
  assert.equal(error.removed, true);
});

test("sample errors are disabled by default", () => {
  assert.ok(source("main.js").includes("const PREVIEW_ERRORS = false;"));
  assert.ok(source("main.js").includes("if (PREVIEW_ERRORS) toast("));
});

test("frontend references resolve and each tab has its own panel", () => {
  const html = source("index.html");
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map(match => match[1]);
  assert.equal(new Set(ids).size, ids.length, "duplicate element ID");
  for (const match of source("main.js").matchAll(/\$\("([^"]+)"\)/g))
    assert.ok(ids.includes(match[1]), `missing element: ${match[1]}`);
  for (const page of ["controls", "devices", "settings"]) {
    assert.ok(html.includes(`aria-controls="page-${page}"`));
    assert.ok(html.includes(`id="page-${page}"`));
  }
  const config = JSON.parse(fs.readFileSync(path.join(__dirname, "../src-tauri/tauri.conf.json")));
  const window = config.app.windows[0];
  assert.equal(window.decorations, false);
  assert.equal(window.skipTaskbar, true);
  assert.equal(window.resizable, true);
});
