// Launcher: a grid of big buttons that open apps, URLs, Shortcuts or shell
// commands. The buttons come from
// ~/Library/Application Support/edge-starter/launcher.json and reload when
// that file changes (see src-tauri/src/examples/launcher.rs).
(() => {
  const body = document.querySelector("#launcher .body");
  const { invoke, esc } = window.ex;
  let rev = 0;

  async function render() {
    const c = await invoke("launcher_config").catch((e) => ({ rev: 0, buttons: [], error: String(e) }));
    rev = c.rev;
    body.innerHTML = `
      ${c.error ? `<p class="launch-error">${esc(c.error)}</p>` : ""}
      ${c.buttons.length || c.error ? "" : `<p class="sub">Add buttons in ${esc(c.path)}</p>`}
      <div class="launch-grid">${c.buttons.map((b, i) => `
        <button class="launch ${b.error ? "broken" : ""}" data-i="${i}" data-label="${esc(b.label)}" data-error="${esc(b.error ?? "")}">
          ${b.image ? `<img src="${b.image}" alt="" />` : `<span class="glyph">${esc(b.glyph ?? "")}</span>`}
          <span class="name">${esc(b.label)}</span>
        </button>`).join("")}</div>`;
  }

  // Restart a one-shot CSS animation.
  function flash(btn, cls) {
    btn.classList.remove("fired", "failed");
    void btn.offsetWidth;
    btn.classList.add(cls);
  }

  function fail(btn, why) {
    flash(btn, "failed");
    const name = btn.querySelector(".name");
    name.textContent = why;
    clearTimeout(btn._restore);
    btn._restore = setTimeout(() => {
      btn.classList.remove("failed");
      name.textContent = btn.dataset.label;
    }, 4000);
  }

  body.onclick = async (e) => {
    const btn = e.target.closest(".launch");
    if (!btn) return;
    if (btn.dataset.error) return fail(btn, btn.dataset.error);
    flash(btn, "fired");
    try {
      await invoke("launcher_run", { rev, index: Number(btn.dataset.i) });
    } catch (err) {
      if (btn.isConnected) fail(btn, String(err));
    }
  };

  render();
  window.__TAURI__.event.listen("launcher-changed", render);
})();
