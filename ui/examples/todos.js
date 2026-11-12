// To-dos: Apple Reminders (EventKit). Open reminders from every list, due
// ones first; tap a row to check it off (tap again to undo), tap the field
// and type to add to your default list. macOS asks once for access.
// Uses the same Rust commands as the Reminders example (reminders.rs).
(() => {
  const body = document.querySelector("#todos .body");
  const { invoke, esc } = window.ex;
  let list = null; // the <ul>, once access is granted
  let busy = 0; // taps still being saved; don't redraw under the finger

  const day = (offset) => {
    const d = new Date();
    d.setDate(d.getDate() + offset);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  };
  const clock = (hhmm) => {
    const [h, m] = hhmm.split(":").map(Number);
    return `${h % 12 || 12}:${String(m).padStart(2, "0")} ${h < 12 ? "AM" : "PM"}`;
  };
  /** "Today 3:00 PM", "Tomorrow", "Nov 20"; overdue is flagged. */
  function due(x) {
    if (!x.due) return { text: "", late: false };
    const today = day(0);
    const now = new Date();
    const hhmm = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
    const late = !x.completed && (x.due < today || (x.due === today && !!x.dueTime && x.dueTime < hhmm));
    const [y, m, d] = x.due.split("-").map(Number);
    const name = x.due === today ? "Today" : x.due === day(1) ? "Tomorrow" : x.due === day(-1) ? "Yesterday"
      : new Date(y, m - 1, d).toLocaleDateString(undefined, { month: "short", day: "numeric" });
    return { text: x.dueTime ? `${name} ${clock(x.dueTime)}` : name, late };
  }

  function setUp() {
    body.innerHTML = `
      <form class="todo-add">
        <input placeholder="Add a to-do" autocomplete="off" />
        <button class="todo-plus" aria-label="Add">+</button>
      </form>
      <ul class="todo-list"></ul>`;
    list = body.querySelector(".todo-list");
    const input = body.querySelector("input");
    body.querySelector("form").onsubmit = async (e) => {
      e.preventDefault();
      const title = input.value.trim();
      if (!title) {
        // "+" with nothing typed: put the cursor in the field.
        invoke("focus_window");
        input.focus();
        return;
      }
      input.value = "";
      await invoke("reminder_add", { title }).catch(() => { input.value = title; });
      refresh();
    };
    list.onclick = async (e) => {
      const li = e.target.closest("li[data-id]");
      if (!li) return;
      const completed = !li.classList.contains("done");
      li.classList.toggle("done", completed); // show it right away
      busy++;
      await invoke("reminder_set_completed", { id: li.dataset.id, completed }).catch(() => {});
      busy--;
      refresh();
    };
  }

  async function refresh() {
    const r = await invoke("reminders_list").catch(() => null);
    if (!r || busy) return;
    if (r.access !== "granted") {
      list = null;
      if (r.access === "unknown") {
        body.innerHTML = `<button class="todo-allow">Show my Reminders</button><p class="sub">macOS will ask once.</p>`;
        body.querySelector("button").onclick = async () => { await invoke("reminders_request_access"); refresh(); };
      } else {
        body.innerHTML = `<p class="sub">Allow Reminders for this app in System Settings > Privacy & Security > Reminders.</p>`;
      }
      return;
    }
    if (!list) setUp();
    const top = list.scrollTop;
    list.innerHTML = r.reminders.map((x) => {
      const d = due(x);
      const meta = [d.text && `<span class="${d.late ? "late" : ""}">${esc(d.text)}</span>`, esc(x.list)].filter(Boolean).join(" · ");
      return `<li role="button" data-id="${esc(x.id)}" class="${x.completed ? "done" : ""}">
        <span class="ring"></span><span class="text"><span class="title">${esc(x.title)}</span><small>${meta}</small></span></li>`;
    }).join("") || `<li class="empty">All caught up.</li>`;
    list.scrollTop = top;
  }

  window.todos = { refresh };
  window.ex.every(15000, refresh);
})();
