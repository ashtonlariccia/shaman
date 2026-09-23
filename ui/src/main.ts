import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import { RAIL_WIDTH } from "./lib/layout";
import "./app.css";
import "./dialog.css";

// The collapsed sidebar's width, as a length the stylesheet can use -- the
// title bar offsets its menus by it. Set here rather than in an effect so it
// is in place before the first paint; from an effect the menus would render
// once at the far left and then jump.
document.documentElement.style.setProperty("--rail-width", `${RAIL_WIDTH}px`);

// Frontend errors are otherwise invisible: WebView2 has no console we can read
// from the outside, so a thrown component error just silently renders nothing.
// Forward them to Rust's log, which `verify.sh` already captures.
function report(detail: string) {
  void invoke("ui_ready", { detail }).catch(() => {});
}

window.addEventListener("error", (event) => {
  report(`JS_ERROR ${event.message} @ ${event.filename}:${event.lineno}:${event.colno}`);
});

window.addEventListener("unhandledrejection", (event) => {
  report(`JS_REJECTION ${String(event.reason)}`);
});

const target = document.getElementById("app");
if (!target) throw new Error("missing #app mount point");

export default mount(App, { target });
