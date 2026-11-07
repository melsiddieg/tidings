<script lang="ts">
  import { untrack } from 'svelte';
  import Modal from './Modal.svelte';
  import { store } from '../lib/store.svelte';
  import { formatDay, parseDay } from '../lib/filters';

  // Start from the current custom range, or from the last 7 days.
  const current = untrack(() => store.dateRange);
  const today = new Date();
  let from = $state(current.preset === 'custom' && current.from ? current.from : formatDay(new Date(today.getFullYear(), today.getMonth(), today.getDate() - 6)));
  let to = $state(current.preset === 'custom' && current.to ? current.to : formatDay(today));

  const fromDay = $derived(parseDay(from));
  const toDay = $derived(parseDay(to));
  const backwards = $derived(!!fromDay && !!toDay && fromDay > toDay);

  function apply(e: SubmitEvent) {
    e.preventDefault();
    if (backwards) return;
    store.setDateRange({ preset: 'custom', from: fromDay ? from : null, to: toDay ? to : null });
    store.dialog = null;
  }
</script>

<Modal title="Custom date range" width={400}>
  <form id="date-range" class="form" onsubmit={apply}>
    <p class="hint">Show articles published between these days. Leave one empty for an open range.</p>
    <div class="grid2">
      <div class="field">
        <label for="dr-from">From</label>
        <input id="dr-from" class="input" type="date" bind:value={from} max={to || undefined} />
      </div>
      <div class="field">
        <label for="dr-to">To (including)</label>
        <input id="dr-to" class="input" type="date" bind:value={to} min={from || undefined} />
      </div>
    </div>
    {#if backwards}<p class="error">“From” is after “To”.</p>{/if}
  </form>
  {#snippet footer()}
    <button class="btn" type="button" onclick={() => (store.dialog = null)}>Cancel</button>
    <button class="btn primary" type="submit" form="date-range" disabled={backwards}>Apply</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: grid;
    gap: 12px;
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .hint {
    margin: 0;
    color: var(--text-2);
    font-size: 13px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 12.5px;
  }
</style>
