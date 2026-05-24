// AirPods Helper — Frontend
// Uses Tauri's global invoke (withGlobalTauri: true in tauri.conf.json)

const { invoke } = window.__TAURI__.core;

// State tracking to avoid redundant UI updates
let lastState = null;
let userInteracting = false;

// ---- Polling ----

async function pollStatus() {
  try {
    const status = await invoke("get_status");
    updateUI(status);
    lastState = status;
  } catch (e) {
    console.error("poll error:", e);
  }
}

setInterval(pollStatus, 1000);
pollStatus();

// ---- UI Update ----

function updateUI(s) {
  // Connection status
  const dot = document.getElementById("status-dot");
  const connText = document.getElementById("connection-text");
  // Only the cards AFTER battery AND AFTER devices-card get the disabled treatment
  const cards = document.querySelectorAll("#battery-section ~ .card:not(#devices-card)");

  const badge = document.getElementById("connection-badge");
  const devicesCard = document.getElementById("devices-card");
  const disconnectBtn = document.getElementById("btn-disconnect");
  if (s.connected) {
    dot.classList.add("connected");
    badge.classList.add("is-connected");
    connText.textContent = "Connected";
    cards.forEach((c) => c.classList.remove("disabled"));
    document.getElementById("battery-section").classList.remove("disabled");
    // Hide devices list, show disconnect button
    devicesCard.classList.add("hidden");
    disconnectBtn.classList.remove("hidden");
  } else {
    dot.classList.remove("connected");
    badge.classList.remove("is-connected");
    connText.textContent = "Disconnected";
    cards.forEach((c) => c.classList.add("disabled"));
    document.getElementById("battery-section").classList.add("disabled");
    // Show devices list, hide disconnect button
    devicesCard.classList.remove("hidden");
    disconnectBtn.classList.add("hidden");
    // Refresh the list on transition into disconnected (not on every poll)
    if (lastState && lastState.connected) {
      refreshPairedDevices();
    }
  }

  // Header
  const name = s.model_name || "AirPods";
  document.getElementById("device-name").textContent = name;

  const modelEl = document.getElementById("device-model");
  const fwEl = document.getElementById("device-firmware");
  modelEl.textContent = s.model ? s.model : "";
  fwEl.textContent = s.firmware ? `FW ${s.firmware}` : "";

  // Battery
  updateBattery("left", s.battery_left, s.charging_left);
  updateBattery("right", s.battery_right, s.charging_right);
  updateBattery("case", s.battery_case, s.charging_case);

  // Feature-based section visibility
  const features = s.features || [];
  const has = (f) => features.includes(f);

  const ancCard = document.getElementById("anc-card");
  if (ancCard) ancCard.classList.toggle("hidden", !has("anc"));

  const caRow = document.getElementById("toggle-ca")?.closest(".toggle-row");
  if (caRow) caRow.classList.toggle("hidden", !has("ca"));

  const obRow = document.getElementById("toggle-one-bud")?.closest(".toggle-row");
  if (obRow) obRow.classList.toggle("hidden", !has("one_bud_anc"));

  // ANC mode
  document.querySelectorAll(".anc-btn").forEach((btn) => {
    if (btn.dataset.mode === s.anc_mode) {
      btn.classList.add("active");
    } else {
      btn.classList.remove("active");
    }
    // Hide adaptive button if model doesn't support it
    if (btn.dataset.mode === "adaptive") {
      btn.classList.toggle("hidden", !has("adaptive"));
    }
  });

  // Adaptive noise slider visibility
  const adaptiveRow = document.getElementById("adaptive-noise-row");
  if (has("adaptive") && s.anc_mode === "adaptive") {
    adaptiveRow.classList.remove("hidden");
  } else {
    adaptiveRow.classList.add("hidden");
  }

  // Update slider value only if user is not dragging
  if (!userInteracting) {
    const slider = document.getElementById("adaptive-noise-slider");
    slider.value = s.adaptive_noise_level;
    document.getElementById("adaptive-noise-value").textContent =
      s.adaptive_noise_level;
  }

  // Feature toggles (only update if not interacting)
  if (!userInteracting) {
    document.getElementById("toggle-ca").checked =
      s.conversational_awareness;
    document.getElementById("toggle-one-bud").checked = s.one_bud_anc;
    document.getElementById("toggle-volume-swipe").checked = s.volume_swipe;
    document.getElementById("toggle-auto-reconnect").checked =
      s.auto_reconnect;
    document.getElementById("toggle-start-login").checked = s.start_on_login;
    document.getElementById("toggle-ear-pause").checked =
      s.ear_detection_pause;
    document.getElementById("toggle-ear-resume").checked =
      s.ear_detection_resume;
    const preferredEl = document.getElementById("preferred-mac");
    if (document.activeElement !== preferredEl) {
      preferredEl.value = s.preferred_device || "";
    }
  }

  // EQ preset
  document.querySelectorAll(".eq-btn").forEach((btn) => {
    if (btn.dataset.preset === s.eq_preset) {
      btn.classList.add("active");
    } else {
      btn.classList.remove("active");
    }
  });

  // Ear status
  const leftDot = document.getElementById("ear-left-dot");
  const rightDot = document.getElementById("ear-right-dot");
  if (s.ear_left) {
    leftDot.classList.add("in-ear");
  } else {
    leftDot.classList.remove("in-ear");
  }
  if (s.ear_right) {
    rightDot.classList.add("in-ear");
  } else {
    rightDot.classList.remove("in-ear");
  }
}

function updateBattery(side, level, charging) {
  const bar = document.getElementById(`battery-${side}-bar`);
  const pct = document.getElementById(`battery-${side}-pct`);
  const chg = document.getElementById(`battery-${side}-charging`);

  if (level < 0) {
    bar.style.width = "0%";
    bar.className = "battery-bar";
    pct.textContent = "--";
    pct.classList.add("unavailable");
    chg.textContent = "";
    return;
  }

  bar.style.width = level + "%";
  bar.className = "battery-bar";
  if (level <= 15) {
    bar.classList.add("low");
  } else if (level <= 30) {
    bar.classList.add("medium");
  }

  pct.textContent = level + "%";
  pct.classList.remove("unavailable");
  chg.textContent = charging ? "Charging" : "";
}

// ---- Event Handlers ----

// ANC buttons
document.querySelectorAll(".anc-btn").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const mode = btn.dataset.mode;
    try {
      await invoke("set_anc_mode", { mode });
      // Optimistic update
      document
        .querySelectorAll(".anc-btn")
        .forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");

      // Show/hide adaptive slider
      const adaptiveRow = document.getElementById("adaptive-noise-row");
      if (mode === "adaptive") {
        adaptiveRow.classList.remove("hidden");
      } else {
        adaptiveRow.classList.add("hidden");
      }
    } catch (e) {
      console.error("set ANC mode error:", e);
    }
  });
});

// Adaptive noise slider
const adaptiveSlider = document.getElementById("adaptive-noise-slider");
adaptiveSlider.addEventListener("mousedown", () => {
  userInteracting = true;
});
adaptiveSlider.addEventListener("touchstart", () => {
  userInteracting = true;
});
adaptiveSlider.addEventListener("input", () => {
  document.getElementById("adaptive-noise-value").textContent =
    adaptiveSlider.value;
});
adaptiveSlider.addEventListener("change", async () => {
  userInteracting = false;
  try {
    await invoke("set_adaptive_noise_level", {
      level: parseInt(adaptiveSlider.value),
    });
  } catch (e) {
    console.error("set adaptive noise level error:", e);
  }
});
adaptiveSlider.addEventListener("mouseup", () => {
  userInteracting = false;
});
adaptiveSlider.addEventListener("touchend", () => {
  userInteracting = false;
});

// Feature toggles
function setupToggle(id, commandName, argName) {
  const el = document.getElementById(id);
  el.addEventListener("change", async () => {
    userInteracting = true;
    try {
      const args = {};
      args[argName] = el.checked;
      await invoke(commandName, args);
    } catch (e) {
      console.error(`${commandName} error:`, e);
      // Revert on error
      el.checked = !el.checked;
    }
    setTimeout(() => {
      userInteracting = false;
    }, 500);
  });
}

setupToggle("toggle-ca", "set_conversational_awareness", "enabled");
setupToggle("toggle-one-bud", "set_one_bud_anc", "enabled");
setupToggle("toggle-volume-swipe", "set_volume_swipe", "enabled");
setupToggle("toggle-auto-reconnect", "set_auto_reconnect", "enabled");
setupToggle("toggle-start-login", "set_start_on_login", "enabled");
setupToggle("toggle-ear-pause", "set_ear_detection_pause", "enabled");
setupToggle("toggle-ear-resume", "set_ear_detection_resume", "enabled");

// Preferred-device MAC pin
const preferredInput = document.getElementById("preferred-mac");
const preferredHint = document.getElementById("preferred-hint");
const clearPreferredBtn = document.getElementById("btn-clear-preferred");

async function savePreferredDevice(value) {
  preferredHint.classList.remove("is-error");
  try {
    await invoke("set_preferred_device", { address: value });
    preferredHint.textContent = value
      ? `Pinned ${value}. The daemon will prefer this device.`
      : "Cleared — daemon will auto-pick any paired AirPods.";
  } catch (e) {
    console.error("set_preferred_device error:", e);
    preferredHint.classList.add("is-error");
    preferredHint.textContent = String(e);
  }
}

preferredInput.addEventListener("change", () => {
  userInteracting = true;
  savePreferredDevice(preferredInput.value.trim().toUpperCase());
  setTimeout(() => (userInteracting = false), 500);
});
preferredInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") preferredInput.blur();
});
clearPreferredBtn.addEventListener("click", async () => {
  preferredInput.value = "";
  await savePreferredDevice("");
});

// Mic mode buttons
document.querySelectorAll(".mic-btn").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const mode = btn.dataset.mic;
    try {
      await invoke("set_mic_mode", { mode });
      document
        .querySelectorAll(".mic-btn")
        .forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
    } catch (e) {
      console.error("set mic mode error:", e);
    }
  });
});

// EQ buttons
document.querySelectorAll(".eq-btn").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const preset = btn.dataset.preset;
    try {
      await invoke("set_eq_preset", { preset });
      document
        .querySelectorAll(".eq-btn")
        .forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
    } catch (e) {
      console.error("set EQ preset error:", e);
    }
  });
});

// ---- Devices (paired list) ----

async function refreshPairedDevices() {
  const list = document.getElementById("device-list");
  const empty = document.getElementById("device-list-empty");
  try {
    const devices = await invoke("list_paired");
    list.innerHTML = "";
    if (!devices || devices.length === 0) {
      empty.classList.remove("hidden");
      return;
    }
    empty.classList.add("hidden");
    for (const d of devices) {
      const li = document.createElement("li");
      li.className = "device-row";

      const info = document.createElement("div");
      info.className = "device-info";
      const name = document.createElement("div");
      name.className = "device-row-name";
      name.textContent = d.name || "AirPods";
      const mac = document.createElement("div");
      mac.className = "device-row-mac";
      mac.textContent = d.address;
      info.appendChild(name);
      info.appendChild(mac);

      const btn = document.createElement("button");
      btn.className = "btn-primary device-row-connect";
      btn.textContent = d.connected ? "Connected" : "Connect";
      if (d.connected) {
        btn.disabled = true;
      } else {
        btn.addEventListener("click", async () => {
          btn.disabled = true;
          btn.textContent = "Connecting...";
          try {
            await invoke("connect", { address: d.address });
            // Poll will pick up the connected state and switch the UI
          } catch (e) {
            console.error("connect error:", e);
            btn.disabled = false;
            btn.textContent = "Connect";
            alert("Connect failed: " + e);
          }
        });
      }

      li.appendChild(info);
      li.appendChild(btn);
      list.appendChild(li);
    }
  } catch (e) {
    console.error("list_paired error:", e);
    list.innerHTML = "";
    empty.classList.remove("hidden");
    empty.textContent = "Failed to list paired devices: " + e;
  }
}

// Disconnect button in header
document.getElementById("btn-disconnect").addEventListener("click", async () => {
  const btn = document.getElementById("btn-disconnect");
  btn.disabled = true;
  btn.textContent = "Disconnecting...";
  try {
    await invoke("disconnect");
  } catch (e) {
    console.error("disconnect error:", e);
    alert("Disconnect failed: " + e);
  } finally {
    btn.disabled = false;
    btn.textContent = "Disconnect";
  }
});

// Refresh button on devices card
document
  .getElementById("btn-refresh-devices")
  .addEventListener("click", refreshPairedDevices);

// Manual pair form
const pairBtn = document.getElementById("btn-pair");
const pairInput = document.getElementById("pair-mac");
const pairHint = document.getElementById("pair-hint");

function isValidMac(mac) {
  return /^[0-9A-Fa-f]{2}(:[0-9A-Fa-f]{2}){5}$/.test(mac.trim());
}

async function attemptPair() {
  const address = pairInput.value.trim().toUpperCase();
  if (!isValidMac(address)) {
    pairHint.textContent = "Invalid MAC — expected AA:BB:CC:DD:EE:FF";
    pairHint.classList.add("is-error");
    return;
  }
  pairHint.classList.remove("is-error");
  pairBtn.disabled = true;
  pairInput.disabled = true;
  pairBtn.textContent = "Pairing...";
  pairHint.textContent = "Waiting for AirPods (up to 20s). Keep the case open.";
  try {
    await invoke("pair", { address });
    pairHint.textContent = `Paired ${address}. You can connect now.`;
    pairInput.value = "";
    await refreshPairedDevices();
  } catch (e) {
    console.error("pair error:", e);
    pairHint.classList.add("is-error");
    pairHint.textContent = "Pair failed: " + e;
  } finally {
    pairBtn.disabled = false;
    pairInput.disabled = false;
    pairBtn.textContent = "Pair";
  }
}

pairBtn.addEventListener("click", attemptPair);
pairInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") attemptPair();
});

// Quick-pair scan
const quickpairBtn = document.getElementById("btn-quickpair");
const quickpairHint = document.getElementById("quickpair-hint");
const candidateList = document.getElementById("candidate-list");

async function runQuickPair() {
  quickpairBtn.disabled = true;
  quickpairBtn.textContent = "Scanning...";
  quickpairHint.textContent = "Scanning for 10 seconds — keep the case open.";
  quickpairHint.classList.remove("is-error");
  candidateList.classList.add("hidden");
  candidateList.innerHTML = "";
  try {
    const candidates = await invoke("quick_pair_scan", { durationSecs: 10 });
    if (!candidates || candidates.length === 0) {
      quickpairHint.textContent =
        "No AirPods found. Open the case (status light should flash white) and scan again.";
      return;
    }
    quickpairHint.textContent = `Found ${candidates.length} candidate${candidates.length === 1 ? "" : "s"}. Tap one to pair.`;
    candidateList.classList.remove("hidden");
    for (const c of candidates) {
      const li = document.createElement("li");
      li.className = "candidate-row";
      if (c.in_pair_mode) li.classList.add("in-pair-mode");

      const info = document.createElement("div");
      info.className = "device-info";
      const name = document.createElement("div");
      name.className = "device-row-name";
      name.textContent = c.model + (c.in_pair_mode ? " ★" : "");
      const meta = document.createElement("div");
      meta.className = "device-row-mac";
      meta.textContent = `${c.address}  ·  ${c.rssi} dBm`;
      info.appendChild(name);
      info.appendChild(meta);

      const btn = document.createElement("button");
      btn.className = "btn-primary candidate-pair";
      btn.textContent = "Pair";
      btn.addEventListener("click", async () => {
        btn.disabled = true;
        btn.textContent = "Pairing...";
        try {
          await invoke("pair", { address: c.address });
          btn.textContent = "Paired";
          quickpairHint.textContent = `Paired ${c.model}. Connecting...`;
          await refreshPairedDevices();
          // Try to auto-connect after pair succeeds
          try {
            await invoke("connect", { address: c.address });
          } catch (e) {
            console.warn("auto-connect after pair failed:", e);
          }
        } catch (e) {
          console.error("pair error:", e);
          btn.disabled = false;
          btn.textContent = "Pair";
          quickpairHint.classList.add("is-error");
          quickpairHint.textContent = "Pair failed: " + e;
        }
      });

      li.appendChild(info);
      li.appendChild(btn);
      candidateList.appendChild(li);
    }
  } catch (e) {
    console.error("quick_pair_scan error:", e);
    quickpairHint.classList.add("is-error");
    quickpairHint.textContent = "Scan failed: " + e;
  } finally {
    quickpairBtn.disabled = false;
    quickpairBtn.textContent = "Scan";
  }
}

quickpairBtn.addEventListener("click", runQuickPair);

// Initial load
refreshPairedDevices();
