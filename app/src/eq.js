// EQ frequency-response math + SVG plot.
//
// Filters follow Robert Bristow-Johnson's "Audio EQ Cookbook" (the same
// biquads PipeWire's filter-chain `bq_*` builtins implement), evaluated at
// the daemon's 48 kHz sample rate.
"use strict";

(function () {
  const FS = 48000;
  const F_MIN = 20;
  const F_MAX = 20000;
  const SVG_NS = "http://www.w3.org/2000/svg";

  const FILTER_TYPES = [
    ["peaking", "Peak"],
    ["lowshelf", "Low shelf"],
    ["highshelf", "High shelf"],
    ["lowpass", "Low-pass"],
    ["highpass", "High-pass"],
    ["notch", "Notch"],
  ];

  /** RBJ biquad coefficients, normalised so a0 = 1. */
  function coefficients(band) {
    const A = Math.pow(10, band.gain / 40);
    const w0 = (2 * Math.PI * band.freq) / FS;
    const cos = Math.cos(w0);
    const alpha = Math.sin(w0) / (2 * band.q);
    const sqA = 2 * Math.sqrt(A) * alpha;
    let b0, b1, b2, a0, a1, a2;
    switch (band.type) {
      case "lowshelf":
        b0 = A * (A + 1 - (A - 1) * cos + sqA);
        b1 = 2 * A * (A - 1 - (A + 1) * cos);
        b2 = A * (A + 1 - (A - 1) * cos - sqA);
        a0 = A + 1 + (A - 1) * cos + sqA;
        a1 = -2 * (A - 1 + (A + 1) * cos);
        a2 = A + 1 + (A - 1) * cos - sqA;
        break;
      case "highshelf":
        b0 = A * (A + 1 + (A - 1) * cos + sqA);
        b1 = -2 * A * (A - 1 + (A + 1) * cos);
        b2 = A * (A + 1 + (A - 1) * cos - sqA);
        a0 = A + 1 - (A - 1) * cos + sqA;
        a1 = 2 * (A - 1 - (A + 1) * cos);
        a2 = A + 1 - (A - 1) * cos - sqA;
        break;
      case "lowpass":
        b0 = (1 - cos) / 2;
        b1 = 1 - cos;
        b2 = (1 - cos) / 2;
        a0 = 1 + alpha;
        a1 = -2 * cos;
        a2 = 1 - alpha;
        break;
      case "highpass":
        b0 = (1 + cos) / 2;
        b1 = -(1 + cos);
        b2 = (1 + cos) / 2;
        a0 = 1 + alpha;
        a1 = -2 * cos;
        a2 = 1 - alpha;
        break;
      case "notch":
        b0 = 1;
        b1 = -2 * cos;
        b2 = 1;
        a0 = 1 + alpha;
        a1 = -2 * cos;
        a2 = 1 - alpha;
        break;
      case "peaking":
      default:
        b0 = 1 + alpha * A;
        b1 = -2 * cos;
        b2 = 1 - alpha * A;
        a0 = 1 + alpha / A;
        a1 = -2 * cos;
        a2 = 1 - alpha / A;
    }
    return [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
  }

  /** |H(e^jw)| in dB for one biquad at frequency f. */
  function magnitudeDb(c, f) {
    const w = (2 * Math.PI * f) / FS;
    const c1 = Math.cos(w), s1 = Math.sin(w);
    const c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
    const nr = c[0] + c[1] * c1 + c[2] * c2;
    const ni = -(c[1] * s1 + c[2] * s2);
    const dr = 1 + c[3] * c1 + c[4] * c2;
    const di = -(c[3] * s1 + c[4] * s2);
    const mag2 = (nr * nr + ni * ni) / (dr * dr + di * di);
    return 10 * Math.log10(Math.max(mag2, 1e-12));
  }

  function validBand(b) {
    return (
      b && Number.isFinite(b.freq) && Number.isFinite(b.q) && Number.isFinite(b.gain) &&
      b.freq > 0 && b.freq < FS / 2 && b.q > 0
    );
  }

  /**
   * Combined response of all bands sampled on a log grid. The preamp is a
   * flat offset (headroom against clipping), so it is left out by default to
   * show the preset's shape; pass `withPreamp` for the absolute level.
   */
  function response(preset, points = 180, withPreamp = false) {
    const bands = (preset && preset.bands ? preset.bands : []).filter(validBand);
    const coeffs = bands.map(coefficients);
    const preamp = withPreamp && preset && Number.isFinite(preset.preamp) ? preset.preamp : 0;
    const out = [];
    const lmin = Math.log10(F_MIN), lmax = Math.log10(F_MAX);
    for (let i = 0; i < points; i++) {
      const f = Math.pow(10, lmin + ((lmax - lmin) * i) / (points - 1));
      let db = preamp;
      for (const c of coeffs) db += magnitudeDb(c, f);
      out.push([f, db]);
    }
    return out;
  }

  function el(name, attrs) {
    const node = document.createElementNS(SVG_NS, name);
    for (const [k, v] of Object.entries(attrs || {})) node.setAttribute(k, String(v));
    return node;
  }

  /**
   * Draw the response of `preset` into `svg` (viewBox 0 0 360 132).
   * `preset` null → flat, muted line (EQ off).
   */
  function plot(svg, preset, { muted = false } = {}) {
    const W = 360, H = 132, padL = 26, padR = 4, padT = 6, padB = 16;
    const pw = W - padL - padR, ph = H - padT - padB;
    const data = response(preset);
    const peak = data.reduce((m, [, db]) => Math.max(m, Math.abs(db)), 0);
    const range = peak <= 12 ? 12 : peak <= 18 ? 18 : 24;
    const lmin = Math.log10(F_MIN), lmax = Math.log10(F_MAX);
    const x = (f) => padL + ((Math.log10(f) - lmin) / (lmax - lmin)) * pw;
    const y = (db) => padT + ((range - Math.max(-range, Math.min(range, db))) / (2 * range)) * ph;

    svg.replaceChildren();
    const grid = el("g", { class: "eq-grid" });
    for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
      grid.append(el("line", { x1: x(f), x2: x(f), y1: padT, y2: padT + ph }));
    }
    const step = range / 2;
    for (let db = -range; db <= range; db += step) {
      grid.append(el("line", { x1: padL, x2: padL + pw, y1: y(db), y2: y(db), class: db === 0 ? "zero" : "" }));
    }
    svg.append(grid);

    const labels = el("g", { class: "eq-labels" });
    for (const [f, t] of [[100, "100"], [1000, "1k"], [10000, "10k"]]) {
      const tx = el("text", { x: x(f), y: H - 3, "text-anchor": "middle" });
      tx.textContent = t;
      labels.append(tx);
    }
    for (const db of [range, 0, -range]) {
      const ty = el("text", { x: padL - 4, y: y(db) + 3.5, "text-anchor": "end" });
      ty.textContent = db > 0 ? `+${db}` : String(db);
      labels.append(ty);
    }
    svg.append(labels);

    let d = "";
    data.forEach(([f, db], i) => {
      d += `${i ? "L" : "M"}${x(f).toFixed(2)},${y(db).toFixed(2)}`;
    });
    const area = `${d}L${x(F_MAX).toFixed(2)},${y(0).toFixed(2)}L${x(F_MIN).toFixed(2)},${y(0).toFixed(2)}Z`;
    svg.append(el("path", { d: area, class: muted ? "eq-area muted" : "eq-area" }));
    svg.append(el("path", { d, class: muted ? "eq-line muted" : "eq-line" }));

    // Band markers (centre frequency at the curve).
    if (!muted && preset && preset.bands) {
      const dots = el("g", { class: "eq-dots" });
      for (const b of preset.bands.filter(validBand)) {
        if (b.freq < F_MIN || b.freq > F_MAX) continue;
        const nearest = data.reduce((best, p) =>
          Math.abs(Math.log(p[0] / b.freq)) < Math.abs(Math.log(best[0] / b.freq)) ? p : best);
        dots.append(el("circle", { cx: x(b.freq), cy: y(nearest[1]), r: 2.6 }));
      }
      svg.append(dots);
    }

    const max = data.reduce((m, p) => (p[1] > m[1] ? p : m), data[0]);
    const min = data.reduce((m, p) => (p[1] < m[1] ? p : m), data[0]);
    return { max, min };
  }

  function formatHz(f) {
    return f >= 1000 ? `${+(f / 1000).toFixed(f >= 10000 ? 0 : 1)} kHz` : `${Math.round(f)} Hz`;
  }

  window.AirPodsEq = { FILTER_TYPES, response, plot, formatHz, coefficients, magnitudeDb };
})();
