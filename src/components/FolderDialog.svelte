<script lang="ts">
  import { untrack } from 'svelte';
  import Modal from './Modal.svelte';
  import { api, errorText } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import type { Dialog } from '../lib/types';

  let { dialog }: { dialog: Extract<Dialog, { kind: 'folder' }> } = $props();

  const { folderId, parentId } = untrack(() => dialog);
  const existing = folderId != null ? store.folderById.get(folderId) : undefined;
  const parent = parentId != null ? store.folderById.get(parentId) : undefined;
  let title = $state(existing?.title ?? '');
  let error = $state<string | null>(null);
  let busy = $state(false);

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!title.trim()) return;
    busy = true;
    try {
      if (existing) await api.renameFolder(existing.id, title.trim());
      else await api.createFolder(title.trim(), parentId);
      await store.reloadTree();
      store.dialog = null;
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={existing ? 'Rename folder' : parent ? `New folder in “${parent.title}”` : 'New folder'} width={400}>
  <form id="folder-form" class="field" onsubmit={save}>
    <label for="folder-name">Name</label>
    <input id="folder-name" class="input" bind:value={title} placeholder="e.g. Technology" autocomplete="off" />
    {#if error}<p class="hint" style:color="var(--danger)">{error}</p>{/if}
  </form>
  {#snippet footer()}
    <button class="btn" type="button" onclick={() => (store.dialog = null)}>Cancel</button>
    <button class="btn primary" type="submit" form="folder-form" disabled={busy || !title.trim()}>{existing ? 'Rename' : 'Create'}</button>
  {/snippet}
</Modal>
