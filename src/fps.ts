// ctrl+shift+f: frames per second, counted off requestAnimationFrame. it asks for every frame while it's on, so it
// shows what the display link actually delivers (180 on a 180hz screen) rather than what the page happens to need
let el: HTMLElement | null = null;
let raf = 0;

export function toggleFps() {
  if (el) { cancelAnimationFrame(raf); el.remove(); el = null; return; }
  el = document.createElement("div");
  el.className = "fps";
  el.textContent = "… fps";
  document.body.append(el);
  let frames = 0, since = performance.now();
  const tick = (now: number) => {
    frames++;
    if (now - since >= 1000) {
      if (el) el.textContent = `${Math.round((frames * 1000) / (now - since))} fps`;
      frames = 0; since = now;
    }
    raf = requestAnimationFrame(tick);
  };
  raf = requestAnimationFrame(tick);
}
