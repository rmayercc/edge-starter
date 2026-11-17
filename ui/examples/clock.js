// Clock: a large time with the date under it. Pure JS, no Rust.
(() => {
  const body = document.querySelector("#clock .body");
  body.innerHTML = `<p class="clock-time"><span></span><small></small></p><p class="clock-date"></p>`;
  const [time, ampm] = body.querySelectorAll(".clock-time > *");
  const date = body.querySelector(".clock-date");
  const dateFmt = new Intl.DateTimeFormat(undefined, { weekday: "long", month: "long", day: "numeric" });
  function tick() {
    const now = new Date();
    const h = now.getHours();
    time.textContent = `${h % 12 || 12}:${String(now.getMinutes()).padStart(2, "0")}`;
    ampm.textContent = h < 12 ? "AM" : "PM";
    date.textContent = dateFmt.format(now);
    // Wake up just after the next minute starts.
    setTimeout(tick, 60000 - (now.getSeconds() * 1000 + now.getMilliseconds()) + 50);
  }
  tick();
})();
