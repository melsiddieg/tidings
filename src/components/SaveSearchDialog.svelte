<script lang="ts">
  import { untrack } from 'svelte';
  import Modal from './Modal.svelte';
  import { store } from '../lib/store.svelte';
  import type { Dialog } from '../lib/types';

  let { dialog }: { dialog: Extract<Dialog, { kind: 'save-search' }> } = $props();

  const id = untrack(() => dialog.id);
  let title = $state(untrack(() => dialog.title));
  let busy = $state(false);

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!title.trim()) return;
    busy = true;
    if (await store.nameSearch(id, title.trim())) store.dialog = null;
    busy = false;
  }
</script>

<Modal title={id == null ? 'Save search' : 'Rename search'} width={420}>
  <form id="search-form" class="field" onsubmit={save}>
    <label for="search-name">Name</label>
    <!-- svelte-ignore a11y_autofocus -->
    <input id="search-name" class="input" bind:value={title} maxlength="100" autocomplete="off" autofocus />
    {#if id == null}
      <p class="hint">
        The sidebar keeps this list: the feed or folder, the read filter, the search words, the dates and the type. It shows
        how many unread articles it finds.
      </p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn" type="button" onclick={() => (store.dialog = null)}>Cancel</button>
    <button class="btn primary" type="submit" form="search-form" disabled={busy || !title.trim()}>{id == null ? 'Save' : 'Rename'}</button>
  {/snippet}
</Modal>
