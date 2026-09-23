/**
 * What kind of terminal something is, decided in one place.
 *
 * Every surface that shows a terminal — the sidebar, the menus, the pinned
 * strip, the dialogs — colours it by kind, and they must all agree. Deriving
 * the kind here rather than re-testing `elevated` and `ssh` in each component is
 * what keeps "blue means local" true everywhere instead of nearly everywhere.
 *
 * The colours themselves live in `app.css` as `--kind-*`.
 */
export type Kind = "local" | "admin" | "remote";

/**
 * A remote tab is remote even though `elevated` is false on it — the two are
 * not a hierarchy, and SSH sessions simply do not carry local privilege.
 */
export function slotKind(slot: { ssh: unknown | null; elevated: boolean }): Kind {
  if (slot.ssh) return "remote";
  return slot.elevated ? "admin" : "local";
}

/** A detected shell: the admin variants are separate profiles of their own. */
export function profileKind(profile: { elevated: boolean }): Kind {
  return profile.elevated ? "admin" : "local";
}

/** Spelled out for tooltips and `title` attributes, so colour is never the only cue. */
export const KIND_TITLE: Record<Kind, string> = {
  local: "Local terminal",
  admin: "Local terminal, running as administrator",
  remote: "Remote connection over SSH",
};
