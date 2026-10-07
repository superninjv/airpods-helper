// AirPods Helper — frontend.
//
// State comes from the Rust side as a full snapshot: once via `get_status`
// and then on every change via the `status` event (no polling). Everything the
// user does goes through `invoke`, and the UI then follows the daemon's
// PropertiesChanged-driven status rather than trusting its own guesses.
"use strict";

(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const Eq = window.AirPodsEq;

  const $ = (id) => document.getElementById(id);
  const SVG_NS = "http://www.w3.org/2000/svg";

  /** Last status snapshot from the backend. */
  let S = null;
  let firstRender = true;

  // ── Utilities ──────────────────────────────────────────────────────────

  function errText(e) {
    if (typeof e === "string") return e;
    if (e && typeof e.message === "string") return e.message;
    return String(e);
  }

  function h(tag, props, ...children) {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(props || {})) {
      if (v == null || v === false) continue;
      if (k === "class") node.className = v;
      else if (k === "text") node.textContent = v;
      else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
      else node.setAttribute(k, v === true ? "" : String(v));
    }
    for (const c of children) if (c != null) node.append(c);
    return node;
  }

  function icon(id, cls) {
    const svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("aria-hidden", "true");
    if (cls) svg.setAttribute("class", cls);
    const use = document.createElementNS(SVG_NS, "use");
    use.setAttribute("href", `#${id}`);
    svg.append(use);
    return svg;
  }

  function debounce(fn, ms) {
    let t = null;
    const wrapped = (...args) => {
      clearTimeout(t);
      t = setTimeout(() => fn(...args), ms);
    };
    wrapped.flush = (...args) => {
      clearTimeout(t);
      fn(...args);
    };
    wrapped.cancel = () => clearTimeout(t);
    return wrapped;
  }

  const has = (f) => !!S && Array.isArray(S.features) && S.features.includes(f);
  const isHeadphones = () => has("headphones");
  const isLinux = () => !S || S.platform !== "windows";
  const isRunning = () => !!S && S.daemon === "running";

  const MAC_RE = /^[0-9A-F]{2}(:[0-9A-F]{2}){5}$/;
  const normMac = (v) => v.trim().toUpperCase();

  // Models with stem touch controls (volume swipe). `Features` has no entry
  // for it yet, so fall back to model numbers (AirPods Pro 2 / Pro 3).
  const SWIPE_MODELS = new Set([
    "A2698", "A2699", "A2700", "A2931", "A2968", "A3047", "A3048", "A3049",
    "A3063", "A3064", "A3065", "A3122",
  ]);
  const supportsSwipe = () =>
    has("volume_swipe") || (!isHeadphones() && SWIPE_MODELS.has(S.model));

  // ── Toasts ─────────────────────────────────────────────────────────────

  function toast(message, kind = "error") {
    const box = $("toasts");
    const close = h("button", { class: "icon-btn small", type: "button", "aria-label": "Dismiss" }, icon("i-close"));
    const node = h(
      "div",
      { class: `toast toast-${kind}`, role: kind === "error" ? "alert" : "status" },
      h("span", { class: "toast-text", text: message }),
      close,
    );
    const remove = () => {
      node.classList.add("leaving");
      setTimeout(() => node.remove(), 180);
    };
    close.addEventListener("click", remove);
    box.append(node);
    while (box.children.length > 3) box.firstElementChild.remove();
    setTimeout(remove, kind === "error" ? 7000 : 3500);
  }

  /** invoke() with an error toast; rethrows so callers can roll back. */
  async function call(cmd, args) {
    try {
      return await invoke(cmd, args);
    } catch (e) {
      toast(errText(e));
      throw e;
    }
  }

  // ── Radio groups (segmented controls & chips) ──────────────────────────

  /**
   * Keyboard behaviour for a role=radiogroup: arrows/Home/End move focus
   * and select, following the WAI-ARIA radio group pattern.
   */
  function wireRadioGroup(group, onSelect) {
    group.addEventListener("keydown", (e) => {
      const radios = [...group.querySelectorAll('[role="radio"]:not([hidden]):not([disabled])')];
      const i = radios.indexOf(document.activeElement);
      if (i < 0) return;
      let next = null;
      if (e.key === "ArrowRight" || e.key === "ArrowDown") next = radios[(i + 1) % radios.length];
      else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = radios[(i - 1 + radios.length) % radios.length];
      else if (e.key === "Home") next = radios[0];
      else if (e.key === "End") next = radios[radios.length - 1];
      if (!next) return;
      e.preventDefault();
      next.focus();
      onSelect(next.dataset.value);
    });
    group.addEventListener("click", (e) => {
      const btn = e.target.closest('[role="radio"]');
      if (btn && !btn.disabled) onSelect(btn.dataset.value);
    });
  }

  function setChecked(group, value) {
    const radios = [...group.querySelectorAll('[role="radio"]')];
    let any = false;
    for (const r of radios) {
      const on = r.dataset.value === value;
      r.setAttribute("aria-checked", on ? "true" : "false");
      any = any || (on && !r.hidden);
    }
    // Roving tabindex: the checked (or first visible) radio is tabbable.
    const visible = radios.filter((r) => !r.hidden);
    for (const r of radios) r.tabIndex = -1;
    const tabbable = any ? radios.find((r) => r.dataset.value === value) : visible[0];
    if (tabbable) tabbable.tabIndex = 0;
  }

  // ── Header ─────────────────────────────────────────────────────────────

  function renderHeader(s) {
    $("header-glyph").setAttribute("href", isHeadphones() ? "#i-headphones" : "#i-airpods");
    const pill = $("conn-pill");
    let state, text, name, sub;
    if (s.daemon === "absent") {
      state = "offline"; text = "Offline"; name = "AirPods Helper"; sub = "Daemon not running";
    } else if (s.daemon === "starting") {
      state = "busy"; text = "Starting…"; name = "AirPods Helper"; sub = "Connecting to airpods-daemon…";
    } else if (!s.connected) {
      state = "idle"; text = "Disconnected"; name = "AirPods"; sub = "No AirPods connected";
    } else {
      state = "on"; text = "Connected";
      name = s.model_name || "AirPods";
      const bits = [];
      if (s.model) bits.push(s.model);
      if (s.firmware) bits.push(`Firmware ${s.firmware}`);
      sub = bits.join(" · ") || "Connected";
    }
    pill.dataset.state = state;
    $("conn-text").textContent = text;
    $("device-name").textContent = name;
    $("device-sub").textContent = sub;
    $("btn-disconnect").hidden = !(isRunning() && s.connected && isLinux());
    document.title = s.connected ? `${name} — AirPods Helper` : "AirPods Helper";
  }

  // ── Daemon card ────────────────────────────────────────────────────────

  function renderDaemon(s) {
    $("daemon-card").hidden = s.daemon !== "absent";
    // The generic "not running" reason just repeats the title; show anything
    // more specific (activation failure, no session bus, …).
    const detail = /is not running\.?$/.test(s.daemon_error || "") ? "" : s.daemon_error || "";
    const err = $("daemon-error");
    err.hidden = !detail;
    err.textContent = detail;
    $("outdated-banner").hidden = !(isRunning() && s.daemon_outdated);
    if (!isLinux()) {
      $("daemon-title").textContent = "airpods-windows isn't running";
      $("cmd-enable").textContent = "airpods-windows daemon";
    }
  }

  // ── Battery ────────────────────────────────────────────────────────────

  function batteryTile(label, level, charging, ear) {
    const known = Number.isFinite(level) && level >= 0;
    const tier = !known ? "unknown" : level <= 15 ? "low" : level <= 30 ? "mid" : "ok";
    const aria = [
      `${label}: ${known ? `${level}%` : "unknown"}`,
      charging ? "charging" : null,
      ear === true ? "in ear" : ear === false ? "not in ear" : null,
    ].filter(Boolean).join(", ");

    const fill = h("div", { class: "meter-fill" });
    fill.style.width = known ? `${level}%` : "0%";
    const pct = h("div", { class: "bat-pct" }, known ? String(level) : "—");
    if (known) pct.append(h("span", { class: "unit", text: "%" }));
    if (charging) pct.append(icon("i-bolt", "bolt"));

    const top = h("div", { class: "bat-top" }, h("span", { class: "bat-name", text: label }));
    if (ear !== undefined) {
      top.append(h("span", { class: `ear-chip${ear ? " in" : ""}`, text: ear ? "In ear" : "Out" }));
    }
    return h(
      "div",
      { class: `bat bat-${tier}${charging ? " charging" : ""}`, role: "group", "aria-label": aria },
      top, pct, h("div", { class: "meter", "aria-hidden": "true" }, fill),
    );
  }

  function renderBattery(s) {
    const show = isRunning() && s.connected;
    $("battery-card").hidden = !show;
    if (!show) return;
    const grid = $("battery-grid");
    const tiles = [];
    if (isHeadphones()) {
      const level = Math.max(s.battery_left, s.battery_right);
      tiles.push(batteryTile("Battery", level, s.charging_left || s.charging_right));
      $("battery-title").textContent = "Battery";
    } else {
      tiles.push(batteryTile("Left", s.battery_left, s.charging_left, s.ear_left));
      tiles.push(batteryTile("Right", s.battery_right, s.charging_right, s.ear_right));
      tiles.push(batteryTile("Case", s.battery_case, s.charging_case));
      $("battery-title").textContent = "Battery & fit";
    }
    grid.classList.toggle("single", isHeadphones());
    grid.replaceChildren(...tiles);
  }

  // ── Listening mode ─────────────────────────────────────────────────────

  const ANC_MODES = [
    { id: "off", label: "Off", path: "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16ZM6.3 6.3l11.4 11.4" },
    { id: "noise", label: "Noise Cancel", path: "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16Zm0 4a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z" },
    { id: "transparency", label: "Transparency", path: "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16Zm0 5.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Z" },
    { id: "adaptive", label: "Adaptive", path: "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16Zm0 0v16" },
  ];
  let ancPending = null;

  function buildAnc() {
    const group = $("anc-group");
    for (const m of ANC_MODES) {
      const svg = document.createElementNS(SVG_NS, "svg");
      svg.setAttribute("viewBox", "0 0 24 24");
      svg.setAttribute("aria-hidden", "true");
      const p = document.createElementNS(SVG_NS, "path");
      p.setAttribute("d", m.path);
      svg.append(p);
      group.append(h("button", {
        type: "button", role: "radio", "aria-checked": "false", "data-value": m.id, class: "seg",
      }, svg, h("span", { text: m.label })));
    }
    wireRadioGroup(group, async (mode) => {
      if (!S || mode === S.anc_mode || ancPending) return;
      ancPending = mode;
      setChecked(group, mode);
      renderAdaptive(mode);
      try {
        await call("set_anc_mode", { mode });
      } catch {
        /* rolled back below */
      } finally {
        ancPending = null;
        renderAnc(S);
      }
    });
  }

  function renderAnc(s) {
    const show = isRunning() && s.connected && has("anc");
    $("anc-card").hidden = !show;
    if (!show) return;
    const group = $("anc-group");
    for (const btn of group.children) {
      btn.hidden = btn.dataset.value === "adaptive" && !has("adaptive");
    }
    const visible = [...group.children].filter((b) => !b.hidden).length;
    group.style.setProperty("--cols", String(visible));
    const mode = ancPending || s.anc_mode;
    setChecked(group, mode);
    renderAdaptive(mode);
  }

  // Adaptive level: debounced while dragging, flushed on release.
  let sliderActiveUntil = 0;
  const sendAdaptive = debounce((level) => {
    call("set_adaptive_noise_level", { level }).catch(() => {});
  }, 250);

  function renderAdaptive(mode) {
    const show = has("adaptive") && mode === "adaptive";
    $("adaptive-row").hidden = !show;
    $("adaptive-hint").hidden = !show;
    if (show && Date.now() > sliderActiveUntil && S) {
      $("adaptive-slider").value = String(S.adaptive_noise_level);
      $("adaptive-value").textContent = String(S.adaptive_noise_level);
      paintSlider();
    }
  }

  function paintSlider() {
    const sl = $("adaptive-slider");
    sl.style.setProperty("--fill", `${sl.value}%`);
  }

  function wireAdaptive() {
    const sl = $("adaptive-slider");
    sl.addEventListener("input", () => {
      sliderActiveUntil = Date.now() + 1500;
      $("adaptive-value").textContent = sl.value;
      paintSlider();
      sendAdaptive(Number(sl.value));
    });
    sl.addEventListener("change", () => {
      sliderActiveUntil = Date.now() + 800;
      sendAdaptive.flush(Number(sl.value));
    });
  }

  // ── Controls (feature toggles + mic) ───────────────────────────────────

  const TOGGLES = [
    { el: "t-ca", row: "row-ca", cmd: "set_conversational_awareness", field: "conversational_awareness", show: () => has("ca") },
    { el: "t-onebud", row: "row-onebud", cmd: "set_one_bud_anc", field: "one_bud_anc", show: () => has("one_bud_anc") },
    { el: "t-swipe", row: "row-swipe", cmd: "set_volume_swipe", field: "volume_swipe", show: supportsSwipe },
  ];
  const pendingToggles = new Set();

  function wireToggles() {
    for (const t of TOGGLES) {
      const input = $(t.el);
      input.addEventListener("change", async () => {
        pendingToggles.add(t.el);
        try {
          await call(t.cmd, { enabled: input.checked });
        } catch {
          input.checked = !!(S && S[t.field]);
        } finally {
          pendingToggles.delete(t.el);
        }
      });
    }
    const mic = $("mic-group");
    for (const [id, label] of [["auto", "Auto"], ["left", "Left"], ["right", "Right"]]) {
      mic.append(h("button", { type: "button", role: "radio", "aria-checked": "false", "data-value": id, class: "seg", text: label }));
    }
    let micPending = null;
    wireRadioGroup(mic, async (mode) => {
      if (!S || mode === S.mic_mode || micPending) return;
      micPending = mode;
      setChecked(mic, mode);
      try {
        await call("set_mic_mode", { mode });
      } catch {
        /* fall through to re-render */
      } finally {
        micPending = null;
        setChecked(mic, S.mic_mode);
      }
    });
  }

  function renderFeatures(s) {
    const connected = isRunning() && s.connected;
    let any = false;
    for (const t of TOGGLES) {
      const show = connected && t.show();
      $(t.row).hidden = !show;
      any = any || show;
      if (show && !pendingToggles.has(t.el)) $(t.el).checked = !!s[t.field];
    }
    const showMic = connected && !isHeadphones() && isLinux();
    $("row-mic").hidden = !showMic;
    if (showMic) setChecked($("mic-group"), s.mic_mode || "auto");
    const showMicSource = renderMicSource(s, connected && isLinux());
    $("features-card").hidden = !(any || showMic || showMicSource);
  }

  // MicStatus → [badge label, tone]. "off" (setting disabled, no session, or a
  // daemon too old to have a mic source) hides the row instead of showing a badge.
  const MIC_BADGES = {
    idle: ["Ready", "neutral"],
    starting: ["Starting", "warn"],
    streaming: ["In use", "ok"],
    unavailable: ["Unavailable", "neutral"],
    error: ["Error", "bad"],
  };

  // Show the state of the "AirPods Microphone" source while connected.
  // Returns whether the row is visible, so the Controls card knows to show.
  function renderMicSource(s, connected) {
    const status = s.mic_status || "off";
    const show = connected && status !== "off";
    $("row-micsrc").hidden = !show;
    const msg = $("mic-message");
    let text = "";
    if (show) {
      const [label, tone] = MIC_BADGES[status] || [status, "neutral"];
      const badge = $("mic-badge");
      badge.textContent = label;
      badge.dataset.tone = tone;
      if (status === "unavailable") text = s.mic_error || "The AirPods microphone can't be offered on this system.";
      else if (status === "error") text = s.mic_error || "The AirPods microphone stopped working.";
    }
    msg.hidden = !text;
    msg.textContent = text;
    msg.dataset.tone = "bad";
    return show;
  }

  // ── Equalizer ──────────────────────────────────────────────────────────

  const presetCache = new Map(); // id -> detail
  let presetFetch = null; // id currently being fetched
  let eqPending = null;
  let editor = null; // { mode: "new"|"edit", draft }

  const EQ_BADGES = {
    off: ["Off", "neutral"],
    active: ["Active", "ok"],
    waiting: ["Waiting", "warn"],
    error: ["Error", "bad"],
    unsupported: ["Unavailable", "neutral"],
  };
  const BACKENDS = {
    pipewire: "PipeWire",
    "pipewire-legacy": "PipeWire (legacy)",
    pulseaudio: "PulseAudio",
  };

  function presetInfo(id) {
    return (S && S.eq_presets.find((p) => p.id === id)) || null;
  }

  const presetFailed = new Set(); // ids whose details couldn't be read

  async function loadPreset(id, force) {
    if (!id || presetFetch === id) return;
    if (!force && (presetCache.has(id) || presetFailed.has(id))) return;
    presetFetch = id;
    try {
      presetCache.set(id, await invoke("get_eq_preset", { id }));
      presetFailed.delete(id);
    } catch (e) {
      presetFailed.add(id);
      console.warn("get_eq_preset failed", e);
    } finally {
      presetFetch = null;
    }
    renderEq(S);
  }

  function renderEqChips(s) {
    const chips = $("eq-chips");
    const selected = eqPending !== null ? eqPending : s.eq_preset;
    const want = ["", ...s.eq_presets.map((p) => p.id)].join("\u0000");
    if (chips.dataset.ids !== want) {
      chips.dataset.ids = want;
      const items = [{ id: "", name: "Off", description: "Turn the equalizer off" }, ...s.eq_presets];
      chips.replaceChildren(...items.map((p) =>
        h("button", {
          type: "button", role: "radio", class: "chip", "data-value": p.id,
          title: p.description || null, "aria-checked": "false", text: p.name || p.id,
        })));
    }
    setChecked(chips, selected);
  }

  function renderEq(s) {
    const show = isRunning() && isLinux();
    $("eq-card").hidden = !show;
    if (!show) return;

    renderEqChips(s);
    const [label, tone] = EQ_BADGES[s.eq_status] || [s.eq_status, "neutral"];
    const badge = $("eq-badge");
    badge.textContent = label;
    badge.dataset.tone = tone;

    const info = presetInfo(s.eq_preset);
    const name = info ? info.name : s.eq_preset;
    const msg = $("eq-message");
    let text = "", tone2 = "info";
    if (s.eq_status === "error") {
      text = s.eq_error ? `Couldn't start the EQ: ${s.eq_error}` : "The EQ filter could not be started.";
      tone2 = "bad";
    } else if (s.eq_status === "unsupported") {
      text = s.eq_error || "No supported audio server (PipeWire or PulseAudio) was found.";
      tone2 = "bad";
    } else if (s.eq_status === "waiting") {
      text = s.connected
        ? `“${name}” will start as soon as the AirPods audio output appears.`
        : `“${name}” will be applied when your AirPods connect.`;
      tone2 = "warn";
    } else if (s.eq_status === "active" && s.eq_backend === "pipewire-legacy") {
      text = "Older WirePlumber: the EQ is your default output while it's on.";
    }
    msg.hidden = !text;
    msg.textContent = text;
    msg.dataset.tone = tone2;

    // Chips are locked while editing, and unusable without an audio backend.
    const unusable = s.eq_status === "unsupported";
    for (const c of $("eq-chips").children) {
      c.disabled = !!editor || (unusable && c.dataset.value !== "");
    }

    // Curve: editor draft > selected preset > flat.
    const svg = $("eq-curve");
    const caption = $("eq-caption");
    let detail = null;
    if (editor) {
      detail = editor.draft;
    } else if (s.eq_preset) {
      detail = presetCache.get(s.eq_preset) || null;
      if (!detail) loadPreset(s.eq_preset);
    }
    if (detail) {
      const { max, min } = Eq.plot(svg, detail);
      const n = detail.bands.length;
      caption.textContent =
        `${editor ? "Preview" : detail.name} · ${n} band${n === 1 ? "" : "s"} · preamp ${fmtDb(detail.preamp)}`;
      svg.setAttribute("aria-label",
        `Frequency response of ${detail.name || "preset"}: from ${fmtDb(min[1])} at ${Eq.formatHz(min[0])} ` +
        `to ${fmtDb(max[1])} at ${Eq.formatHz(max[0])}`);
    } else {
      Eq.plot(svg, null, { muted: true });
      caption.textContent = !s.eq_preset
        ? "EQ off — flat response"
        : presetFailed.has(s.eq_preset) ? "Couldn't read this preset's bands" : "Loading preset…";
      svg.setAttribute("aria-label", "Flat frequency response (EQ off)");
    }

    $("eq-desc").textContent = editor ? "" : (info && info.description) || (BACKENDS[s.eq_backend] ? `via ${BACKENDS[s.eq_backend]}` : "");
    const edit = $("eq-edit");
    edit.hidden = !s.eq_preset || !!editor;
    edit.textContent = info && info.user_editable ? "Edit" : "Customize";
    edit.title = info && info.user_editable ? "Edit this preset" : "Make an editable copy of this preset";
    $("eq-new").hidden = !!editor;
    $("eq-actions").hidden = !!editor;
    $("eq-editor").hidden = !editor;
  }

  function fmtDb(v) {
    const r = Math.round(v * 10) / 10;
    return `${r > 0 ? "+" : r < 0 ? "−" : ""}${Math.abs(r)} dB`;
  }

  function wireEqChips() {
    wireRadioGroup($("eq-chips"), async (id) => {
      if (!S || editor || id === S.eq_preset || eqPending !== null) return;
      eqPending = id;
      renderEq(S);
      if (id) loadPreset(id, true);
      try {
        await call("set_eq_preset", { id });
      } catch {
        /* status re-render restores the real selection */
      } finally {
        eqPending = null;
        renderEq(S);
      }
    });
  }

  // ── Preset editor ──

  function slugify(name) {
    return name.toLowerCase().normalize("NFKD").replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40) || "preset";
  }

  function uniqueId(base, except) {
    const taken = new Set(S.eq_presets.map((p) => p.id));
    if (except) taken.delete(except);
    let id = base.slice(0, 48), n = 2;
    while (taken.has(id)) id = `${base.slice(0, 44)}-${n++}`;
    return id;
  }

  function openEditor(mode) {
    let draft;
    if (mode === "new") {
      draft = {
        id: "", name: "My preset", description: "", preamp: 0,
        bands: [{ type: "peaking", freq: 1000, q: 1, gain: 0 }],
      };
    } else {
      const src = presetCache.get(S.eq_preset);
      if (!src) {
        toast("Preset details are still loading — try again in a moment.", "info");
        return;
      }
      const info = presetInfo(S.eq_preset);
      draft = JSON.parse(JSON.stringify(src));
      if (!(info && info.user_editable)) {
        mode = "copy";
        draft.name = `${src.name} (custom)`;
      }
    }
    editor = { mode, original: mode === "edit" ? S.eq_preset : null, draft, deleteArmed: false };
    syncIdFromName();
    fillEditor();
    renderEq(S);
    $("ed-name").focus();
    $("ed-name").select();
  }

  function closeEditor() {
    editor = null;
    renderEq(S);
    $("eq-new").focus();
  }

  function syncIdFromName() {
    if (editor.mode === "edit") editor.draft.id = editor.original;
    else editor.draft.id = uniqueId(slugify(editor.draft.name || "preset"));
    $("ed-id").textContent = `${editor.draft.id}.toml`;
  }

  function bandRow(band, index) {
    const typeSel = h("select", { "aria-label": `Band ${index + 1} type` },
      ...Eq.FILTER_TYPES.map(([v, l]) => h("option", { value: v, text: l, selected: v === band.type })));
    const num = (key, attrs) => h("input", { type: "number", inputmode: "decimal", value: String(band[key]), "data-key": key, ...attrs });
    const freq = num("freq", { min: 20, max: 20000, step: "any", "aria-label": `Band ${index + 1} frequency in hertz` });
    const q = num("q", { min: 0.1, max: 10, step: 0.1, "aria-label": `Band ${index + 1} Q` });
    const gain = num("gain", { min: -24, max: 24, step: 0.5, "aria-label": `Band ${index + 1} gain in decibels` });
    const remove = h("button", { type: "button", class: "icon-btn small", "aria-label": `Remove band ${index + 1}`, title: "Remove band" }, icon("i-close"));
    const row = h("div", { class: "band", role: "row", "data-index": String(index) }, typeSel, freq, q, gain, remove);
    // Gain has no effect on pass/notch filters.
    gain.disabled = ["lowpass", "highpass", "notch"].includes(band.type);
    typeSel.addEventListener("change", () => {
      band.type = typeSel.value;
      gain.disabled = ["lowpass", "highpass", "notch"].includes(band.type);
      onDraftChanged();
    });
    for (const input of [freq, q, gain]) {
      input.addEventListener("input", () => {
        band[input.dataset.key] = input.value === "" ? NaN : Number(input.value);
        onDraftChanged();
      });
    }
    remove.addEventListener("click", () => {
      editor.draft.bands.splice(index, 1);
      fillBands();
      onDraftChanged();
      const rows = $("ed-bands").children;
      (rows[Math.min(index, rows.length - 1)]?.querySelector("select") || $("ed-add")).focus();
    });
    return row;
  }

  function fillBands() {
    $("ed-bands").replaceChildren(...editor.draft.bands.map(bandRow));
    $("ed-add").disabled = editor.draft.bands.length >= 16;
  }

  function fillEditor() {
    const d = editor.draft;
    $("ed-name").value = d.name;
    $("ed-desc").value = d.description || "";
    $("ed-preamp").value = String(d.preamp);
    $("ed-delete").hidden = editor.mode !== "edit";
    $("ed-delete").textContent = "Delete";
    $("ed-save").textContent = editor.mode === "edit" ? "Save" : "Save & use";
    fillBands();
    validateDraft();
  }

  function validateDraft() {
    const d = editor.draft;
    const errors = [];
    if (!d.name.trim()) errors.push("Give the preset a name.");
    if (!Number.isFinite(d.preamp) || d.preamp < -24 || d.preamp > 12) errors.push("Preamp must be between −24 and +12 dB.");
    if (d.bands.length > 16) errors.push("At most 16 bands.");
    d.bands.forEach((b, i) => {
      const n = i + 1;
      if (!Number.isFinite(b.freq) || b.freq < 20 || b.freq > 20000) errors.push(`Band ${n}: frequency must be 20–20000 Hz.`);
      if (!Number.isFinite(b.q) || b.q < 0.1 || b.q > 10) errors.push(`Band ${n}: Q must be 0.1–10.`);
      if (!Number.isFinite(b.gain) || b.gain < -24 || b.gain > 24) errors.push(`Band ${n}: gain must be −24 to +24 dB.`);
    });
    const box = $("ed-error");
    box.hidden = errors.length === 0;
    box.textContent = errors[0] || "";
    // Mark offending inputs.
    for (const row of $("ed-bands").children) {
      const b = d.bands[Number(row.dataset.index)];
      if (!b) continue;
      row.querySelector('[data-key="freq"]').toggleAttribute("aria-invalid", !(b.freq >= 20 && b.freq <= 20000));
      row.querySelector('[data-key="q"]').toggleAttribute("aria-invalid", !(b.q >= 0.1 && b.q <= 10));
      row.querySelector('[data-key="gain"]').toggleAttribute("aria-invalid", !(b.gain >= -24 && b.gain <= 24));
    }
    $("ed-preamp").toggleAttribute("aria-invalid", !(d.preamp >= -24 && d.preamp <= 12));
    $("ed-save").disabled = errors.length > 0;
    return errors.length === 0;
  }

  const replot = debounce(() => editor && renderEq(S), 16);
  function onDraftChanged() {
    validateDraft();
    replot();
  }

  function wireEditor() {
    $("eq-new").addEventListener("click", () => openEditor("new"));
    $("eq-edit").addEventListener("click", () => openEditor("edit"));
    $("ed-name").addEventListener("input", (e) => {
      editor.draft.name = e.target.value;
      syncIdFromName();
      onDraftChanged();
    });
    $("ed-desc").addEventListener("input", (e) => (editor.draft.description = e.target.value));
    $("ed-preamp").addEventListener("input", (e) => {
      editor.draft.preamp = e.target.value === "" ? NaN : Number(e.target.value);
      onDraftChanged();
    });
    $("ed-add").addEventListener("click", () => {
      const bands = editor.draft.bands;
      if (bands.length >= 16) return;
      const last = bands[bands.length - 1];
      const freq = last ? Math.min(16000, Math.round(last.freq * 2)) : 1000;
      bands.push({ type: "peaking", freq, q: 1, gain: 0 });
      fillBands();
      onDraftChanged();
      $("ed-bands").lastElementChild.querySelector('[data-key="freq"]').focus();
    });
    $("ed-cancel").addEventListener("click", closeEditor);
    $("eq-editor").addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closeEditor();
      }
    });
    $("eq-editor").addEventListener("submit", async (e) => {
      e.preventDefault();
      if (!editor || !validateDraft()) return;
      const ed = editor;
      const d = ed.draft;
      const preset = {
        id: d.id,
        name: d.name.trim(),
        description: (d.description || "").trim(),
        preamp: d.preamp,
        bands: d.bands.map((b) => ({ type: b.type, freq: b.freq, q: b.q, gain: b.gain })),
      };
      const save = $("ed-save");
      save.disabled = true;
      save.textContent = "Saving…";
      try {
        await call("save_eq_preset", { preset });
        presetCache.set(preset.id, preset);
        editor = null;
        if (ed.mode !== "edit" || S.eq_preset !== preset.id) {
          await call("set_eq_preset", { id: preset.id }).catch(() => {});
        }
        toast(`Saved “${preset.name}”.`, "info");
        renderEq(S);
        $("eq-edit").focus();
      } catch {
        if (editor) {
          save.disabled = false;
          save.textContent = ed.mode === "edit" ? "Save" : "Save & use";
        }
      }
    });
    $("ed-delete").addEventListener("click", async () => {
      if (!editor || editor.mode !== "edit") return;
      const btn = $("ed-delete");
      if (!editor.deleteArmed) {
        editor.deleteArmed = true;
        btn.textContent = "Click again to delete";
        setTimeout(() => {
          if (editor && editor.deleteArmed) {
            editor.deleteArmed = false;
            btn.textContent = "Delete";
          }
        }, 4000);
        return;
      }
      const id = editor.original;
      const name = editor.draft.name;
      btn.disabled = true;
      try {
        await call("delete_eq_preset", { id });
        presetCache.delete(id);
        editor = null;
        toast(`Deleted “${name}”.`, "info");
        renderEq(S);
        $("eq-new").focus();
      } catch {
        btn.disabled = false;
      }
    });
  }

  // ── Devices ────────────────────────────────────────────────────────────

  let paired = [];
  let connecting = null;

  async function refreshPaired() {
    if (!isRunning() || !isLinux()) return;
    const btn = $("btn-refresh");
    btn.classList.add("spinning");
    try {
      paired = await invoke("list_paired");
    } catch (e) {
      toast(`Couldn't list paired devices: ${errText(e)}`);
      paired = [];
    } finally {
      btn.classList.remove("spinning");
    }
    renderDevices(S);
    $("paired-options").replaceChildren(...paired.map((d) => h("option", { value: d.address, text: d.name })));
  }

  function deviceRow(name, meta, button) {
    return h("li", { class: "list-row" },
      h("div", { class: "list-text" },
        h("span", { class: "list-name", text: name }),
        h("span", { class: "list-meta", text: meta })),
      button);
  }

  function renderDevices(s) {
    const show = isRunning() && !s.connected && isLinux();
    $("devices-card").hidden = !show;
    if (!show) return;
    $("device-empty").hidden = paired.length > 0;
    $("device-list").replaceChildren(...paired.map((d) => {
      const busy = connecting === d.address;
      const btn = h("button", {
        type: "button", class: "btn btn-primary small", disabled: connecting !== null,
        "aria-label": `Connect ${d.name}`, text: busy ? "Connecting…" : "Connect",
      });
      btn.addEventListener("click", () => connectTo(d.address));
      const preferred = s.preferred_device && s.preferred_device === d.address.toUpperCase();
      return deviceRow(d.name || "AirPods", preferred ? `${d.address} · preferred` : d.address, btn);
    }));
  }

  async function connectTo(address) {
    connecting = address;
    renderDevices(S);
    try {
      await call("connect_device", { address });
    } catch {
      /* toast shown */
    } finally {
      connecting = null;
      renderDevices(S);
    }
  }

  function wireDevices() {
    $("btn-refresh").addEventListener("click", refreshPaired);

    $("btn-disconnect").addEventListener("click", async () => {
      const btn = $("btn-disconnect");
      btn.disabled = true;
      try {
        await call("disconnect_device");
      } catch {
        /* toast shown */
      } finally {
        btn.disabled = false;
      }
    });

    $("btn-scan").addEventListener("click", async () => {
      const btn = $("btn-scan");
      const hint = $("qp-hint");
      const list = $("qp-list");
      const SECONDS = 10;
      btn.disabled = true;
      list.replaceChildren();
      let left = SECONDS;
      btn.textContent = `Scanning… ${left}s`;
      const tick = setInterval(() => {
        left = Math.max(0, left - 1);
        btn.textContent = `Scanning… ${left}s`;
      }, 1000);
      hint.textContent = "Looking for AirPods in pairing mode…";
      try {
        const found = await invoke("quick_pair_scan", { seconds: SECONDS });
        if (!found.length) {
          hint.textContent = "Nothing found. Open the case, hold the setup button until the light flashes white, and scan again.";
        } else {
          hint.textContent = found.some((c) => c.in_pair_mode)
            ? "Found AirPods in pairing mode:"
            : "Found nearby AirPods, but none in pairing mode. Hold the setup button and scan again, or try pairing:";
          list.replaceChildren(...found.map((c) => {
            const btn2 = h("button", { type: "button", class: `btn small${c.in_pair_mode ? " btn-primary" : ""}`, "aria-label": `Pair ${c.model || c.name}`, text: "Pair" });
            btn2.addEventListener("click", () => pairWith(c.address, btn2));
            const meta = `${c.address} · ${c.rssi} dBm${c.in_pair_mode ? " · ready to pair" : ""}`;
            return deviceRow(c.model || c.name || "AirPods", meta, btn2);
          }));
        }
      } catch (e) {
        hint.textContent = "Scan failed.";
        toast(`Scan failed: ${errText(e)}`);
      } finally {
        clearInterval(tick);
        btn.disabled = false;
        btn.textContent = "Scan again";
      }
    });

    $("pair-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      const input = $("pair-mac");
      const mac = normMac(input.value);
      if (!MAC_RE.test(mac)) {
        input.setAttribute("aria-invalid", "true");
        toast("Enter a MAC address like AA:BB:CC:DD:EE:FF.");
        input.focus();
        return;
      }
      input.removeAttribute("aria-invalid");
      await pairWith(mac, $("btn-pair"));
      input.value = "";
    });
    $("pair-mac").addEventListener("input", (e) => e.target.removeAttribute("aria-invalid"));
  }

  async function pairWith(address, btn) {
    const label = btn.textContent;
    btn.disabled = true;
    btn.textContent = "Pairing…";
    try {
      await call("pair_device", { address });
      toast("Paired — connecting…", "info");
      await refreshPaired();
      btn.textContent = "Paired";
      await connectTo(address);
    } catch {
      btn.disabled = false;
      btn.textContent = label;
    }
  }

  // ── Settings ───────────────────────────────────────────────────────────

  const pendingSettings = new Set();

  function renderSettings(s) {
    const show = isRunning() || s.daemon === "absent";
    $("settings-card").hidden = !show;
    const daemonSettings = isRunning() && isLinux();
    for (const input of document.querySelectorAll("[data-setting]")) {
      input.closest(".toggle-row").hidden = !daemonSettings;
      if (!pendingSettings.has(input.id)) input.checked = !!s[input.dataset.setting];
    }
    $("preferred-form").closest(".field-block").hidden = !daemonSettings;
    if (!pendingSettings.has("s-login")) $("s-login").checked = !!s.start_on_login;
    const pref = $("s-preferred");
    if (document.activeElement !== pref && !pendingSettings.has("s-preferred")) pref.value = s.preferred_device || "";
    const bits = [];
    if (s.version) bits.push(`airpods-daemon ${s.version}`);
    if (BACKENDS[s.eq_backend]) bits.push(`audio: ${BACKENDS[s.eq_backend]}`);
    $("versions").textContent = bits.join(" · ");
    $("versions").hidden = bits.length === 0;
  }

  async function savePreferred(value) {
    const input = $("s-preferred");
    const mac = normMac(value);
    if (mac && !MAC_RE.test(mac)) {
      input.setAttribute("aria-invalid", "true");
      toast("Enter a MAC address like AA:BB:CC:DD:EE:FF, or leave it empty.");
      return;
    }
    if (S && mac === (S.preferred_device || "")) return;
    input.removeAttribute("aria-invalid");
    pendingSettings.add("s-preferred");
    try {
      await call("set_setting", { key: "preferred_device", value: mac });
    } catch {
      input.value = (S && S.preferred_device) || "";
    } finally {
      pendingSettings.delete("s-preferred");
    }
  }

  function wireSettings() {
    for (const input of document.querySelectorAll("[data-setting]")) {
      input.addEventListener("change", async () => {
        pendingSettings.add(input.id);
        try {
          await call("set_setting", { key: input.dataset.setting, value: input.checked });
        } catch {
          input.checked = !!(S && S[input.dataset.setting]);
        } finally {
          pendingSettings.delete(input.id);
        }
      });
    }
    $("s-login").addEventListener("change", async (e) => {
      pendingSettings.add("s-login");
      try {
        await call("set_start_on_login", { enabled: e.target.checked });
      } catch {
        e.target.checked = !!(S && S.start_on_login);
      } finally {
        pendingSettings.delete("s-login");
      }
    });
    $("preferred-form").addEventListener("submit", (e) => {
      e.preventDefault();
      $("s-preferred").blur();
    });
    $("s-preferred").addEventListener("change", (e) => savePreferred(e.target.value));
    $("btn-preferred-clear").addEventListener("click", () => {
      $("s-preferred").value = "";
      savePreferred("");
    });
  }

  // ── Daemon card actions ────────────────────────────────────────────────

  function wireDaemonCard() {
    $("btn-retry").addEventListener("click", async () => {
      const btn = $("btn-retry");
      btn.disabled = true;
      btn.textContent = "Starting…";
      try {
        await invoke("retry_daemon");
      } catch (e) {
        toast(errText(e) || "The daemon is still not reachable.");
      } finally {
        btn.disabled = false;
        btn.textContent = "Try again";
      }
    });
    for (const btn of document.querySelectorAll("[data-copy]")) {
      btn.addEventListener("click", async () => {
        const text = $(btn.dataset.copy).textContent;
        try {
          await navigator.clipboard.writeText(text);
          toast("Copied to clipboard.", "info");
        } catch {
          const range = document.createRange();
          range.selectNodeContents($(btn.dataset.copy));
          const sel = window.getSelection();
          sel.removeAllRanges();
          sel.addRange(range);
          toast("Press Ctrl+C to copy the selected command.", "info");
        }
      });
    }
  }

  // ── Render ─────────────────────────────────────────────────────────────

  function render(s) {
    const prev = S;
    S = s;
    renderHeader(s);
    renderDaemon(s);
    renderBattery(s);
    renderAnc(s);
    renderEq(s);
    renderFeatures(s);
    renderDevices(s);
    renderSettings(s);

    // Transitions that need a fetch.
    const nowDisconnected = isRunning() && !s.connected;
    const wasDisconnected = prev && prev.daemon === "running" && !prev.connected;
    if (nowDisconnected && (!wasDisconnected || firstRender)) refreshPaired();
    if (prev && prev.eq_preset !== s.eq_preset && s.eq_preset) loadPreset(s.eq_preset, true);
    if (prev && prev.eq_presets !== s.eq_presets) {
      // Drop cached details of presets that no longer exist.
      for (const id of presetCache.keys()) if (!s.eq_presets.some((p) => p.id === id)) presetCache.delete(id);
    }
    if (firstRender) {
      firstRender = false;
      $("app").setAttribute("aria-busy", "false");
    }
  }

  function wire() {
    buildAnc();
    wireAdaptive();
    wireToggles();
    wireEqChips();
    wireEditor();
    wireDevices();
    wireSettings();
    wireDaemonCard();

    // Pick up presets/devices changed elsewhere (CLI, other clients).
    window.addEventListener("focus", () => {
      if (!isRunning() || !isLinux()) return;
      invoke("refresh_eq_presets").catch(() => {});
      if (S.eq_preset && !editor) loadPreset(S.eq_preset, true);
      if (!S.connected) refreshPaired();
    });
  }

  async function init() {
    wire();
    await listen("status", (e) => render(e.payload));
    await listen("backend-error", (e) => toast(errText(e.payload)));
    try {
      render(await invoke("get_status"));
    } catch (e) {
      toast(`Couldn't read status: ${errText(e)}`);
    }
  }

  init();
})();
