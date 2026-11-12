// Calendar: today's and tomorrow's meetings from the macOS Calendar app
// (EventKit), with a Join button for Teams / Zoom / Meet / Webex links.
// Refreshes every minute and when EventKit says something changed.
// See src-tauri/src/examples/calendar.rs.
(() => {
  const body = document.querySelector("#calendar .body");
  const { invoke, esc } = window.ex;
  const SOON = 5 * 60 * 1000; // Join stands out this long before the start
  let events = null;

  const time = (ms) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const midnight = (offset) => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    d.setDate(d.getDate() + offset);
    return d.getTime();
  };

  function row(e, now) {
    const [start, end] = [e.start * 1000, e.end * 1000];
    const live = start <= now && now < end;
    const soon = !live && start - now <= SOON && start > now;
    const mins = Math.ceil((start - now) / 60000);
    const status = live ? `In progress · ends ${time(end)}`
      : start > now && mins <= 60 ? `Starts in ${mins} min`
      : "";
    const where = e.location || e.calendar;
    return `
      <div class="cal-row ${live ? "live" : ""} ${soon ? "soon" : ""}">
        <div class="cal-time"><b>${time(start)}</b><small>${time(end)}</small></div>
        <div class="cal-what">
          <span class="title">${esc(e.title)}</span>
          <small>${status ? `<span class="status">${status}</span>${where ? " · " : ""}` : ""}${esc(where)}</small>
        </div>
        ${e.join ? `<button class="cal-join ${live || soon ? "hot" : ""}" data-id="${esc(e.id)}">Join</button>` : ""}
      </div>`;
  }

  function draw() {
    if (!events) return;
    const now = Date.now();
    const days = [0, 1].map((offset) => {
      const [lo, hi] = [midnight(offset), midnight(offset + 1)];
      const on = events.filter((e) => e.start * 1000 < hi && e.end * 1000 > lo);
      return {
        name: offset ? "Tomorrow" : "Today",
        date: new Date(lo).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" }),
        allDay: on.filter((e) => e.allDay),
        // Meetings that are over drop off the list.
        timed: on.filter((e) => !e.allDay && e.end * 1000 > now),
      };
    });
    const scroller = body.querySelector(".cal-scroll");
    const top = scroller ? scroller.scrollTop : 0;
    body.innerHTML = `<div class="cal-scroll">${days.map((d) => `
      <h2 class="cal-day">${d.name}<span>${d.date}</span></h2>
      ${d.allDay.length ? `<p class="cal-allday"><span>All day</span>${d.allDay.map((e) => `<b>${esc(e.title)}</b>`).join("")}</p>` : ""}
      ${d.timed.map((e) => row(e, now)).join("") || `<p class="cal-none">${d.name === "Today" ? "Nothing else today." : "Nothing scheduled."}</p>`}`).join("")}</div>`;
    body.querySelector(".cal-scroll").scrollTop = top;
  }

  async function refresh() {
    const r = await invoke("calendar_events").catch(() => null);
    if (!r) return;
    if (r.access === "unknown") {
      events = null;
      body.innerHTML = `<button class="todo-allow">Show my calendar</button><p class="sub">macOS will ask once.</p>`;
      body.querySelector("button").onclick = async () => { await invoke("calendar_request_access"); refresh(); };
    } else if (r.access !== "granted") {
      events = null;
      body.innerHTML = `<p class="sub">Allow Calendars (full access) for this app in System Settings > Privacy & Security > Calendars.</p>`;
    } else {
      events = r.events;
      draw();
    }
  }

  body.onclick = async (e) => {
    const btn = e.target.closest(".cal-join");
    if (!btn) return;
    btn.classList.add("opening");
    try {
      await invoke("calendar_join", { id: btn.dataset.id });
    } catch (err) {
      if (btn.isConnected) btn.textContent = "No link";
    }
    setTimeout(() => btn.classList.remove("opening"), 600);
  };

  // Refetch on the minute; redraw in between so "in progress" stays current.
  refresh();
  (function onTheMinute() {
    setTimeout(() => { refresh(); onTheMinute(); }, 60000 - (Date.now() % 60000) + 200);
  })();
  setInterval(draw, 15000);

  // EventKit changes come in bursts; wait for a quiet moment. To-dos are
  // Reminders, which EventKit reports the same way.
  let quiet;
  window.__TAURI__.event.listen("eventkit-changed", () => {
    clearTimeout(quiet);
    quiet = setTimeout(() => { refresh(); window.todos?.refresh(); }, 400);
  });
})();
