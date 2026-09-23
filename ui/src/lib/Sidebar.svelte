<script lang="ts">
  import { KIND_TITLE, slotKind } from "./kinds";
  import type { Slot } from "./slots";

  type Props = {
    slots: Slot[];
    activeKey: number | null;
    width: number;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
  };

  let { slots, activeKey, width, onselect, onclose }: Props = $props();
</script>

<aside style="width: {width}px">
  <ul>
    {#each slots as slot (slot.key)}
      {@const kind = slotKind(slot)}
      <li>
        <div
          class="row"
          class:active={slot.key === activeKey}
          data-kind={kind}
          role="button"
          tabindex="0"
          onclick={() => onselect(slot.key)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onselect(slot.key)}
        >
          <span class="dot" title={KIND_TITLE[kind]}></span>
          <span class="label">{slot.title}</span>
          <button
            class="kill"
            title="Kill terminal"
            onclick={(e) => {
              e.stopPropagation();
              onclose(slot.key);
            }}
          >
            ×
          </button>
        </div>
      </li>
    {/each}
  </ul>

</aside>

<style>
  aside {
    /* Width is driven by the drag handle in App.svelte. */
    flex: none;
    display: flex;
    flex-direction: column;
    /* Darkest surface in the app, so it recedes behind the terminal. */
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    min-height: 0;
    /* NOTE: no overflow here. `overflow` creates a clipping context, which is
       what was cutting off menus that extended past the sidebar. Scrolling
       belongs on the list itself. */
  }

  ul {
    list-style: none;
    margin: 0;
    /* Inset so hover chips never touch the sidebar's edges. */
    padding: 0.3rem 0.35rem;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  li + li {
    margin-top: 1px;
  }

  /* Each row resolves its kind once, into `--kind` and `--kind-soft`; the dot
     and the selected background both read those rather than repeating the
     three-way choice. */
  .row {
    --kind: var(--kind-local);
    --kind-soft: var(--kind-local-soft);

    display: flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.35rem 0.45rem;
    border-radius: var(--chip-radius);
    cursor: pointer;
    user-select: none;
  }
  .row[data-kind="admin"] {
    --kind: var(--kind-admin);
    --kind-soft: var(--kind-admin-soft);
  }
  .row[data-kind="remote"] {
    --kind: var(--kind-remote);
    --kind-soft: var(--kind-remote-soft);
  }

  .row:hover {
    background: var(--hover);
  }

  /* Selection is tinted with the row's own kind rather than one shared accent,
     so the colour code survives the state that would otherwise paint over it. */
  .row.active {
    background: var(--kind-soft);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--kind);
    flex: none;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.82rem;
  }

  .kill {
    background: transparent;
    border: none;
    color: var(--fg-dim);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    padding: 0 0.15rem;
    border-radius: 3px;
  }
  .kill:hover {
    color: var(--danger);
    background: #f38ba81f;
  }
</style>
