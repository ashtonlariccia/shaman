/**
 * Which monospace fonts this machine actually has.
 *
 * There is no API that lists installed fonts without a permission prompt, so
 * this probes a candidate list instead: render a string in `"<candidate>",
 * <generic>` and compare its width against `<generic>` alone. A different
 * width means the candidate was used, which means it resolved.
 *
 * Three generics are tried rather than one, because a candidate that *is* the
 * browser's default for a generic measures identically to it. Consolas against
 * `monospace` is exactly that case on Windows, and testing only `monospace`
 * would report the most likely font on the machine as missing.
 *
 * The method can still produce a false negative for a font that is metrically
 * identical to all three defaults. That costs a name in a dropdown, not the
 * ability to use it — the dialog lets the font be typed in as well.
 */

/** Wide enough that a metric difference of a fraction of a pixel shows up. */
const PROBE = "mmmmmmmmmmlli0Oo";
const PROBE_SIZE = 72;

const GENERICS = ["monospace", "serif", "sans-serif"] as const;

/**
 * Monospace faces worth asking about: what Windows ships, plus the programming
 * fonts people actually install. Unavailable ones are filtered out, so a name
 * here that nobody has costs nothing.
 */
const CANDIDATES = [
  "Cascadia Code",
  "Cascadia Mono",
  "Consolas",
  "Courier New",
  "Lucida Console",
  "Lucida Sans Typewriter",
  "MS Gothic",
  "NSimSun",
  "Anonymous Pro",
  "Berkeley Mono",
  "CommitMono",
  "Cousine",
  "DejaVu Sans Mono",
  "Fira Code",
  "Fira Mono",
  "Geist Mono",
  "Hack",
  "IBM Plex Mono",
  "Inconsolata",
  "Iosevka",
  "JetBrains Mono",
  "Liberation Mono",
  "Maple Mono",
  "Monaspace Argon",
  "Monaspace Neon",
  "Monaspace Xenon",
  "Noto Sans Mono",
  "PT Mono",
  "Roboto Mono",
  "Source Code Pro",
  "Space Mono",
  "Ubuntu Mono",
  "Victor Mono",
  "0xProto",
];

function widthIn(ctx: CanvasRenderingContext2D, family: string): number {
  ctx.font = `${PROBE_SIZE}px ${family}`;
  return ctx.measureText(PROBE).width;
}

function isAvailable(ctx: CanvasRenderingContext2D, family: string): boolean {
  return GENERICS.some(
    (generic) => widthIn(ctx, `"${family}", ${generic}`) !== widthIn(ctx, generic),
  );
}

/**
 * The candidates this machine can render, alphabetically.
 *
 * `always` is folded in whatever the probe says, so a font already chosen never
 * vanishes from the list that is supposed to be showing it.
 */
export function installedMonospaceFonts(always?: string): string[] {
  const found = new Set<string>();
  if (always?.trim()) found.add(always.trim());

  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) {
    // Can't measure: offer everything rather than an empty dropdown.
    CANDIDATES.forEach((f) => found.add(f));
    return [...found].sort((a, b) => a.localeCompare(b));
  }

  for (const family of CANDIDATES) {
    if (isAvailable(ctx, family)) found.add(family);
  }

  return [...found].sort((a, b) => a.localeCompare(b));
}
