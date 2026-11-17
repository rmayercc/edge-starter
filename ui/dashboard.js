// Dashboard-wide wiring: a warning in the corner while touch isn't working.
(() => {
  const status = document.getElementById("touch-status");
  const MESSAGES = {
    starting: "Starting touch…",
    needsPermission: "Touch is off: allow this app in System Settings > Privacy & Security > Input Monitoring.",
    unsupported: "Touch takeover only works on macOS.",
  };
  async function refresh() {
    const s = await window.__TAURI__?.core?.invoke("touch_status").catch(() => null);
    if (!s) return;
    status.hidden = s.state === "on";
    status.textContent = MESSAGES[s.state] ?? `Touch is off (${s.detail ?? s.state}).`;
  }
  refresh();
  setInterval(refresh, 3000);
})();
