<script lang="ts">
  import Modal from './Modal.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';

  let format = $state<'html' | 'markdown'>('html');
  let images = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function exportFile(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = null;
    try {
      const path = await api.exportStarred(format, images);
      if (path) {
        store.dialog = null;
        store.toast(`Saved starred articles to ${path}`, 'success');
      }
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Export starred articles" width={460}>
  <form id="export-form" class="form" onsubmit={exportFile}>
    <div class="field" role="radiogroup" aria-label="Format">
      <span class="field-label">Format</span>
      <label class="choice"><input type="radio" bind:group={format} value="html" /> HTML, for a web browser</label>
      <label class="choice"><input type="radio" bind:group={format} value="markdown" /> Markdown, for a notes app such as Obsidian</label>
    </div>
    <label class="choice"><input type="checkbox" bind:checked={images} /> Include images</label>
    <p class="hint">
      {#if images}
        Images stay on their websites. They load from there when you open the file, so those sites see that you opened it.
      {:else}
        Images are left out, so opening the file connects to no website.
      {/if}
      The file holds {store.tree.starred.toLocaleString()} starred article{store.tree.starred === 1 ? '' : 's'}, with the full text
      where it was loaded, and only you can read it.
    </p>
    {#if error}<p class="hint error">{error}</p>{/if}
  </form>
  {#snippet footer()}
    <button class="btn" type="button" onclick={() => (store.dialog = null)}>Cancel</button>
    <button class="btn primary" type="submit" form="export-form" disabled={busy || store.tree.starred === 0}>Export…</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: grid;
    gap: 14px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-2);
  }
  .choice input {
    accent-color: var(--accent);
    margin: 0;
  }
  .hint {
    margin: 0;
  }
  .error {
    color: var(--danger, #d14);
  }
</style>
