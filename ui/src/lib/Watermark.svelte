<script lang="ts">
  // Imported raw rather than inlined: a template literal would need every
  // backslash and backtick escaped, and one missed escape silently corrupts the
  // picture. `?raw` keeps it byte-for-byte.
  import waves from "./waves.txt?raw";

  // Trailing newline trimmed so it doesn't add a blank line and skew the
  // vertical centring.
  const ART = waves.replace(/\s+$/, "");
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
    /* 27 lines by 150 columns. At ~0.6em per cell that is 90 character-widths
       across, so it is ~3.3x wider than tall and *width* binds; min() takes
       whichever of height/width runs out first either way.
       
       Deliberately small. The art is dense enough now that the shading reads
       as texture rather than as characters, and at this size it sits behind
       the empty stage instead of filling it. The 12px cap keeps it modest on
       a large monitor rather than growing to match. */
    font-size: clamp(3px, min(2.4vh, 0.55vw), 10px);
    /* 1.0 keeps the character cell close to a terminal's aspect ratio; anything
       taller stretches the drawing vertically. */
    line-height: 1;
    white-space: pre;

    /* The same blue -> lavender -> coral run as the mark in the title bar,
       sampled from it, so the two read as one logo rather than two. Painted
       through the glyphs with background-clip, which keeps the ASCII texture
       instead of replacing it with the image.

       `color: transparent` is what lets the gradient show; without it the
       text paints over its own background. */
    background: linear-gradient(
      95deg,
      #6cb2e8 0%,
      #8fb8ea 20%,
      #cbbdff 50%,
      #ef909c 78%,
      #ee7487 100%
    );
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;

    /* Dimmed so it reads as a watermark rather than a billboard -- but the
       blue end of the gradient is close in value to the terminal background,
       so it cannot go as low as the flat mauve version did before the left
       wave disappears into the surface. */
    opacity: 0.62;
    user-select: none;
  }
</style>
