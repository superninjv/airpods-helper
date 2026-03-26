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
  const cards = document.querySelectorAll("#battery-section ~ .card");

  const badge = document.getElementById("connection-badge");
  if (s.connected) {
    dot.classList.add("connected");
    badge.classList.add("is-connected");
    connText.textContent = "Connected";
    cards.forEach((c) => c.classList.remove("disabled"));
    document.getElementById("battery-section").classList.remove("disabled");
  } else {
    dot.classList.remove("connected");
    badge.classList.remove("is-connected");
    connText.textContent = "Disconnected";
    cards.forEach((c) => c.classList.add("disabled"));
    document.getElementById("battery-section").classList.add("disabled");
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

  // ANC mode
  document.querySelectorAll(".anc-btn").forEach((btn) => {
    if (btn.dataset.mode === s.anc_mode) {
      btn.classList.add("active");
    } else {
      btn.classList.remove("active");
    }
  });

  // Adaptive noise slider visibility
  const adaptiveRow = document.getElementById("adaptive-noise-row");
  if (s.anc_mode === "adaptive") {
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
