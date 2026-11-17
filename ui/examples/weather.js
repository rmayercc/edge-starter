// Weather from Open-Meteo: now, today's high and low, the next hours.
// Your location is in ~/Library/Application Support/edge-starter/weather.json
// (see src-tauri/src/examples/weather.rs).
(() => {
  const body = document.querySelector("#weather .body");
  const { invoke, esc } = window.ex;
  const ICON = { clear: "☀︎", partly: "⛅︎", cloudy: "☁︎", fog: "🌫︎", drizzle: "🌦︎", rain: "🌧︎", snow: "❄︎", storm: "⛈︎" };
  const icon = (kind, isDay = true) => (kind === "clear" && !isDay ? "☾" : ICON[kind] ?? "");
  const hour = (t) => {
    const h = Number(t.slice(11, 13));
    return `${h % 12 || 12}${h < 12 ? "a" : "p"}`;
  };
  let shown = false;
  async function render() {
    let w;
    try {
      w = await invoke("weather");
    } catch (err) {
      // Keep the last reading on screen through a network blip.
      if (!shown) body.innerHTML = `<p class="sub">${esc(typeof err === "string" ? err : "Weather is unavailable right now.")}</p>`;
      return;
    }
    shown = true;
    body.innerHTML = `
      <div class="row"><span class="big-num">${w.temp}°</span><span class="wx-icon">${icon(w.kind, w.isDay)}</span>
        <p class="wx-now">${w.label}<span>${esc(w.name ?? "")}</span></p></div>
      <p class="sub">Feels like ${w.feelsLike}° · High ${w.high}° · Low ${w.low}°</p>
      <div class="hours">${w.hours.filter((_, i) => i % 2 === 0).slice(0, 6).map((h) =>
        `<div><span>${hour(h.time)}</span>${ICON[h.kind] ?? ""}<br />${h.temp}°${h.precipChance >= 30 ? `<span>${h.precipChance}%</span>` : ""}</div>`).join("")}</div>`;
  }
  window.ex.every(15 * 60 * 1000, render);
})();
