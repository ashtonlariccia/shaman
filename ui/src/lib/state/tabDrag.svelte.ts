/**
 * Dragging a terminal out of the sidebar.
 *
 * The gesture spans windows, which rules out both of the obvious mechanisms.
 * HTML5 drag-and-drop cannot leave a webview, and pointer coordinates stop
 * meaning anything the moment the cursor does: the webview takes the mouse
 * capture when the button goes down, so it keeps reporting positions as if the
 * pointer were still inside it, whatever is actually underneath.
 *
 * So the pointer drives the gesture and the OS answers the only question that
 * matters — what is under the cursor — through `drop_target`. That round trip
 * is also what the ghost reads to say where the terminal will land, which is
 * the only feedback available once the cursor is over another window.
 */
import { invoke } from "@tauri-apps/api/core";

/** Pixels of travel before a press becomes a drag rather than a click. */
export const DRAG_SLOP = 5;

/** What releasing here would do. */
export type DropKind =
  /** Back into the window it came from: nothing happens. */
  | "self"
  /** Another Shaman window, which will take the terminal over. */
  | "window"
  /** Anywhere else — a new window opens for it. */
  | "new";

type Target = { label: string | null; x: number; y: number };

export class TabDrag {
  /** The slot being dragged, or null when no drag is in flight. */
  key = $state<number | null>(null);
  /** Past the slop threshold. Until then the press is still a click. */
  started = $state(false);
  /** Ghost position, in viewport coordinates. */
  x = $state(0);
  y = $state(0);
  title = $state("");
  /** Where a release right now would put it. Null until the first probe lands. */
  kind = $state<DropKind | null>(null);

  /** This window's own label, so "over myself" can be told from "over a sibling". */
  #self: string;
  /** Start of the press, for measuring the slop. */
  #from = { x: 0, y: 0 };
  /** One probe in flight at a time: pointer moves outrun any round trip. */
  #probing = false;
  #latest: Target | null = null;

  constructor(self: string) {
    this.#self = self;
  }

  get active(): boolean {
    return this.key !== null && this.started;
  }

  begin(key: number, title: string, event: PointerEvent) {
    this.key = key;
    this.title = title;
    this.started = false;
    this.kind = null;
    this.#latest = null;
    this.#from = { x: event.clientX, y: event.clientY };
    this.x = event.clientX;
    this.y = event.clientY;
  }

  move(event: PointerEvent) {
    if (this.key === null) return;

    this.x = event.clientX;
    this.y = event.clientY;

    if (!this.started) {
      const travel = Math.hypot(event.clientX - this.#from.x, event.clientY - this.#from.y);
      if (travel < DRAG_SLOP) return;
      this.started = true;
    }

    void this.#probe();
  }

  /**
   * Ask the OS what is under the cursor, at most one question at a time.
   *
   * A pointer can report a thousand moves a second; the answer only has to be
   * fresh enough for a ghost label to keep up, so a move that arrives while a
   * probe is out is simply skipped rather than queued.
   */
  async #probe() {
    if (this.#probing) return;
    this.#probing = true;
    try {
      const target = await invoke<Target>("drop_target");
      if (this.key === null) return;
      this.#latest = target;
      this.kind = this.#classify(target);
    } catch (e) {
      console.error("drop_target failed", e);
    } finally {
      this.#probing = false;
    }
  }

  #classify(target: Target): DropKind {
    if (target.label === null) return "new";
    return target.label === this.#self ? "self" : "window";
  }

  /**
   * End the drag and say where it landed.
   *
   * Probes the target one final time rather than trusting the last one in
   * flight: the drop is the only reading that has to be exact.
   */
  async finish(): Promise<{ key: number; target: string | null; x: number; y: number } | null> {
    const key = this.key;
    const started = this.started;
    this.cancel();
    if (key === null || !started) return null;

    let target = this.#latest;
    try {
      target = await invoke<Target>("drop_target");
    } catch (e) {
      console.error("drop_target failed", e);
    }
    if (!target) return null;

    // Dropped back on its own window: the terminal is already there.
    if (target.label === this.#self) return null;

    return { key, target: target.label, x: target.x, y: target.y };
  }

  cancel() {
    this.key = null;
    this.started = false;
    this.kind = null;
  }
}
