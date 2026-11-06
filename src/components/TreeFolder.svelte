<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import TreeFolder from './TreeFolder.svelte';
  import FeedRow from './FeedRow.svelte';
  import { drag, endDrag, startDrag } from '../lib/dnd.svelte';
  import { store } from '../lib/store.svelte';
  import type { FolderNode } from '../lib/types';
  import { countLabel } from '../lib/util';

  let { node, depth }: { node: FolderNode; depth: number } = $props();

  const folder = $derived(node.folder);
  const unread = $derived(store.folderUnread.get(folder.id) ?? 0);
  const selected = $derived(store.scope.kind === 'folder' && store.scope.id === folder.id);
  const dropKey = $derived(`folder:${folder.id}`);

  function menu(e: MouseEvent) {
    store.openMenu(e, [
      { label: 'Refresh', icon: 'refresh', action: () => store.refresh({ folderId: folder.id }) },
      { label: 'Mark all as read', icon: 'check-check', action: () => store.markAllRead({ kind: 'folder', id: folder.id }) },
      { separator: true },
      { label: 'Add feed here…', icon: 'plus', action: () => (store.dialog = { kind: 'add-feed', folderId: folder.id }) },
      { label: 'New subfolder…', icon: 'folder-plus', action: () => (store.dialog = { kind: 'folder', folderId: null, parentId: folder.id }) },
      { label: 'Rename…', icon: 'pencil', action: () => (store.dialog = { kind: 'folder', folderId: folder.id, parentId: folder.parentId }) },
      { separator: true },
      { label: 'Delete folder…', icon: 'trash', danger: true, action: () => store.deleteFolder(folder.id) },
    ]);
  }

  function ondragover(e: DragEvent) {
    const item = drag.item;
    if (!item || (item.type === 'folder' && item.id === folder.id)) return;
    e.preventDefault();
    e.stopPropagation();
    drag.over = dropKey;
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    const item = drag.item;
    endDrag();
    if (!item) return;
    if (item.type === 'feed') store.moveFeed(item.id, folder.id, null);
    else if (item.id !== folder.id) store.moveFolder(item.id, folder.id, null);
  }
</script>

<div
  class="tree-row"
  class:selected
  class:drop-into={drag.over === dropKey}
  style:--depth={depth}
  role="treeitem"
  aria-selected={selected}
  aria-expanded={folder.expanded}
  tabindex="-1"
  draggable="true"
  ondragstart={(e) => startDrag(e, { type: 'folder', id: folder.id })}
  ondragend={endDrag}
  {ondragover}
  ondragleave={() => drag.over === dropKey && (drag.over = null)}
  {ondrop}
  onclick={() => store.selectScope({ kind: 'folder', id: folder.id })}
  ondblclick={() => store.toggleFolder(folder.id)}
  onkeydown={() => {}}
  oncontextmenu={menu}
>
  <button
    class="twisty"
    class:open={folder.expanded}
    aria-label={folder.expanded ? 'Collapse' : 'Expand'}
    onclick={(e) => {
      e.stopPropagation();
      store.toggleFolder(folder.id);
    }}><Icon name="chevron-right" size={12} stroke={2.4} /></button
  >
  <span class="row-icon"><Icon name={folder.expanded ? 'folder-open' : 'folder'} size={15} /></span>
  <span class="row-label">{folder.title}</span>
  {#if unread > 0}<span class="row-count">{countLabel(unread)}</span>{/if}
</div>

{#if folder.expanded}
  {#each node.folders as child (child.folder.id)}
    <TreeFolder node={child} depth={depth + 1} />
  {/each}
  {#each node.feeds as feed (feed.id)}
    <FeedRow {feed} depth={depth + 1} />
  {/each}
{/if}
