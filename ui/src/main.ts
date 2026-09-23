import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import "./app.css";

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
