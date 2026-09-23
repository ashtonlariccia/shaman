<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import { installedMonospaceFonts } from "./fonts";
  import type { AppearanceStore, CursorShape } from "./state/appearance.svelte";

  type Props = {
    open: boolean;
    appearance: AppearanceStore;
    onclose: () => void;
  };

  let { open, appearance, onclose }: Props = $props();

  const a = $derived(appearance.current);

  // Probed once the dialog is first opened rather than at mount: the measuring
  // is ~100 canvas calls, and there is no reason to spend them on a window that
  // may never show this.
  let fonts = $state<string[] | null>(null);
  $effect(() => {
    if (open && fonts === null) fonts = installedMonospaceFonts(a.fontFamily);
  });

  const SHAPES: { value: CursorShape; label: string }[] = [
    { value: "block", label: "Block" },
    { value: "bar", label: "Bar" },
    { value: "underline", label: "Underline" },
  ];

  /** Size is typed as well as stepped, so an empty box mustn't write NaN. */
  function onSizeInput(event: Event) {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    if (Number.isFinite(value)) void appearance.patch({ fontSize: value });
  }
</script>

<Dialog {open} title="Appearance" kind="local" width={460} {onclose}>
  <div class="grid">
    <label for="ap-font">Font</label>
    <select
      id="ap-font"
      value={a.fontFamily}
      onchange={(e) => void appearance.patch({ fontFamily: e.currentTarget.value })}
    >
      {#each fonts ?? [a.fontFamily] as family (family)}
        <option value={family}>{family}</option>
      {/each}
    </select>

    <label for="ap-size">Size</label>
    <div class="row">
      <input
        id="ap-size"
        type="number"
        min="6"
        max="72"
        value={a.fontSize}
        oninput={onSizeInput}
      />
      <span class="unit">px</span>
    </div>

    <span class="label-ish">Cursor</span>
    <div class="segmented" role="group" aria-label="Cursor shape">
      {#each SHAPES as shape (shape.value)}
        <button
          type="button"
          class:selected={a.cursorShape === shape.value}
          onclick={() => void appearance.patch({ cursorShape: shape.value })}
        >
          {shape.label}
        </button>
      {/each}
    </div>

    <label for="ap-cursor-colour">Cursor colour</label>
    <div class="row">
      <input
        id="ap-cursor-colour"
        type="color"
        value={a.cursorColor}
        oninput={(e) => void appearance.patch({ cursorColor: e.currentTarget.value })}
      />
      <span class="swatch-value">{a.cursorColor}</span>
      <label class="check">
        <input
          type="checkbox"
          checked={a.cursorBlink}
          onchange={(e) => void appearance.patch({ cursorBlink: e.currentTarget.checked })}
        />
        Blink
      </label>
    </div>

    <label for="ap-opacity">Background</label>
    <div class="row">
      <!-- Floor is 20%, matching the store's clamp: a window you can't see is
           a state you can't get out of from inside it. -->
      <input
        id="ap-opacity"
        type="range"
        min="20"
        max="100"
        step="1"
        value={a.backgroundOpacity}
        oninput={(e) => void appearance.patch({ backgroundOpacity: Number(e.currentTarget.value) })}
      />
      <span class="unit pct">{a.backgroundOpacity}%</span>
    </div>

    <span class="label-ish">Material</span>
    <label class="check">
      <input
        type="checkbox"
        checked={a.material === "acrylic"}
        onchange={(e) =>
          void appearance.patch({ material: e.currentTarget.checked ? "acrylic" : "none" })}
      />
      Acrylic — frost whatever is behind the window
    </label>
  </div>

  <p class="dlg-note">
    Applies to every terminal, and is saved as you change it. Acrylic needs the background
    below 100% to show through.
  </p>

  <div class="dlg-actions">
    <button class="btn ghost" onclick={() => void appearance.reset()}>Reset</button>
    <button class="btn primary" onclick={onclose}>Done</button>
  </div>
</Dialog>

<style>
  .grid {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 0.6rem 0.75rem;
  }

  label,
  .label-ish {
    font-size: 0.8rem;
    color: var(--fg-dim);
  }

  /* Several controls share a row with their readout. */
  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  select,
  input[type="number"] {
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.82rem;
    padding: 0.3rem 0.4rem;
  }
  select {
    width: 100%;
  }
  input[type="number"] {
    width: 4.5rem;
  }
  select:focus,
  input:focus-visible {
    outline: none;
    border-color: var(--accent);
  }

  .unit {
    font-size: 0.75rem;
    color: var(--fg-dim);
  }
  /* Fixed width so the row doesn't jitter as the number changes width. */
  .unit.pct {
    width: 2.6rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  /* Same two-option toggle idiom as the connect dialog's auth picker. */
  .segmented {
    display: flex;
    gap: 2px;
    padding: 2px;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 999px;
    width: fit-content;
  }

  .segmented button {
    background: transparent;
    border: none;
    border-radius: 999px;
    color: var(--fg-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.76rem;
    padding: 0.22rem 0.7rem;
  }
  .segmented button:hover {
    color: var(--fg);
  }
  .segmented button.selected {
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 600;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.78rem;
    color: var(--fg);
    cursor: pointer;
  }
  .check input {
    accent-color: var(--accent);
    cursor: pointer;
  }

  /* A colour well, not a full-width slab: the native control stretches to
     whatever box it is given. */
  input[type="color"] {
    width: 2.2rem;
    height: 1.55rem;
    padding: 1px;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    cursor: pointer;
  }

  .swatch-value {
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 0.72rem;
    color: var(--fg-dim);
  }

  input[type="range"] {
    flex: 1;
    min-width: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
</style>
