<script lang="ts">
  // Imported raw rather than inlined: a template literal would need every
  // backslash and backtick escaped, and one missed escape silently corrupts the
  // picture. `?raw` keeps it byte-for-byte.
  import shiv from "./shiv.txt?raw";

  // Trailing newline trimmed so it doesn't add a blank line and skew the
  // vertical centring.
  const ART = shiv.replace(/\s+$/, "");
</script>

<div class="blank">
  <pre class="art" aria-hidden="true">{ART}</pre>
</div>

<style>
  .blank {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    /* Decorative only — never intercept clicks headed for the sidebar. */
    pointer-events: none;
  }

  .art {
    margin: 0;
    font-family: "Cascadia Mono", Consolas, "Courier New", monospace;
    /* The art is 14 lines tall by 76 wide. A monospace cell is about 0.6em, so
       it is roughly 3.3x wider than it is tall and *width* is the binding
       constraint -- the opposite of the 54-line version this replaced.
       min() takes whichever of height/width runs out first, so it still fits
       whatever shape the window is. */
    font-size: clamp(3px, min(6vh, 1.9vw), 20px);
    /* 1.0 keeps the character cell close to a terminal's aspect ratio; anything
       taller stretches the drawing vertically. */
    line-height: 1;
    white-space: pre;
    /* Mocha mauve, dimmed so it reads as a watermark rather than a billboard. */
    color: var(--accent);
    opacity: 0.55;
    user-select: none;
  }
</style>
