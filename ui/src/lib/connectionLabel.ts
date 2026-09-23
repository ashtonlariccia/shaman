import type { SavedConnection } from "./types";

/**
 * How a saved connection reads in a list.
 *
 * A named entry leads with the name and shows the address underneath; an
 * unnamed one leads with the address, so nothing is ever a blank row.
 */
export function connectionLines(c: SavedConnection): { primary: string; secondary: string } {
  const named = c.name && c.name.trim().length > 0;
  const primary = named ? c.name! : c.host;

  const parts: string[] = [];
  // Only repeat the address when it isn't already the primary line.
  if (named) parts.push(c.host);
  parts.push(`${c.username}:${c.port}`);

  return { primary, secondary: parts.join(" · ") };
}
