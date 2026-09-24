/**
 * What a right-click menu contains, and where it was asked for.
 *
 * The shape lives here rather than in the component so App can build a menu
 * without importing the thing that draws it, and so the sidebar can hand a
 * menu up to App instead of growing a popover of its own — one menu open at a
 * time is the whole reason this is owned in one place.
 */

export type ContextItem =
  | { kind: "sep" }
  /** `danger` tints the entry red; it is for entries that destroy something. */
  | { kind: "item"; label: string; danger?: boolean; run: () => void };

export type ContextMenuState = {
  /** Viewport coordinates of the click. The menu clamps itself to the window. */
  x: number;
  y: number;
  items: ContextItem[];
};

/** An entry, spelled out so call sites read as a list rather than as objects. */
export function item(label: string, run: () => void, danger = false): ContextItem {
  return { kind: "item", label, danger, run };
}

export const SEP: ContextItem = { kind: "sep" };
