<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import TreeFolder from './TreeFolder.svelte';
  import FeedRow from './FeedRow.svelte';
  import { drag, endDrag, startDrag, zoneFor, zoneOf, type Zone } from '../lib/dnd.svelte';
  import { store } from '../lib/store.svelte';
  import type { FolderNode, ItemRef } from '../lib/types';
  import { childKey, countLabel, sameItem } from '../lib/util';

  let { node, depth }: { node: FolderNode; depth: number } = $props();

  const folder = $derived(node.folder);
  const ref = $derived<ItemRef>({ kind: 'folder', id: node.folder.id });
  const unread = $derived(store.folderUnread.get(folder.id) ?? 0);
  const selected = $derived(store.scope.kind === 'folder' && store.scope.id === folder.id);
  const key = $derived(`folder:${folder.id}`);
  const zone = $derived(zoneFor(key));
  const pinned = $derived(folder.pinned > 0);
  const marked = $derived(store.marked.includes(key));

  function menu(e: MouseEvent) {
    if (marked && store.marked.length > 1) return store.markedMenu(e);
    const inside = store.folderSubtree(folder.id);
    store.openMenu(e, [
      { label: 'Refresh', icon: 'refresh', action: () => store.refresh({ folderId: folder.id }) },
      { label: 'Mark all as read', icon: 'check-check', action: () => store.markAllRead({ kind: 'folder', id: folder.id }) },
      { label: pinned ? 'Remove from Favorites' : 'Add to Favorites', icon: 'star', action: () => store.setPinned(ref, !pinned) },
      { separator: true },
      { label: 'Add feed here…', icon: 'plus', action: () => (store.dialog = { kind: 'add-feed', folderId: folder.id }) },
      { label: 'New subfolder…', icon: 'folder-plus', action: () => (store.dialog = { kind: 'folder', folderId: null, parentId: folder.id }) },
      { label: 'Rename…', icon: 'pencil', action: () => (store.dialog = { kind: 'folder', folderId: folder.id, parentId: folder.parentId }) },
      { separator: true },
      { label: 'Expand all inside', icon: 'expand', disabled: inside.length < 2 && folder.expanded, action: () => store.setExpanded(true, inside) },
      { label: 'Collapse all inside', icon: 'collapse', action: () => store.setExpanded(false, inside) },
      { separator: true },
      { label: 'Delete folder…', icon: 'trash', danger: true, action: () => store.deleteFolder(folder.id) },
    ]);
  }

  /** Top edge: before this folder. Middle: inside it. Bottom edge: after it.
   *  An open folder's children follow its row, so its bottom edge means "inside". */
  function where(e: DragEvent): Zone {
    const z = zoneOf(e, true);
    return z === 'after' && folder.expanded ? 'into' : z;
  }

  function ondragover(e: DragEvent) {
    const item = drag.item;
    if (!item || sameItem(item, ref)) return;
    e.preventDefault();
    e.stopPropagation();
    drag.over = `${key}:${where(e)}`;
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    const item = drag.item;
    const z = where(e);
    endDrag();
    if (!item || sameItem(item, ref)) return;
    if (z === 'into') store.moveItem(item, folder.id, null);
    else if (z === 'before') store.moveItem(item, folder.parentId, ref);
    else store.moveItem(item, folder.parentId, store.itemAfter(folder.parentId, ref, item));
  }
</script>

<div
  class="tree-row"
  class:selected
  class:marked
  class:drop-into={zone === 'into'}
  class:drop-before={zone === 'before'}
  class:drop-after={zone === 'after'}
  style:--depth={depth}
  role="treeitem"
  aria-selected={selected}
  aria-expanded={folder.expanded}
  tabindex="-1"
  draggable="true"
  ondragstart={(e) => startDrag(e, ref)}
  ondragend={endDrag}
  {ondragover}
  ondragleave={() => zone && (drag.over = null)}
  {ondrop}
  onclick={(e) => store.clickItem(ref, e)}
  ondblclick={() => store.toggleFolder(folder.id)}
  onkeydown={() => {}}
  oncontextmenu={menu}
>
  <button
    class="twisty"
    class:open={folder.expanded}
    aria-label={folder.expanded ? 'Collapse' : 'Expand'}
    title="Option-click: also every folder inside"
    onclick={(e) => {
      e.stopPropagation();
      // Option/Alt-click opens or closes the folder and everything inside it.
      if (e.altKey) store.setExpanded(!folder.expanded, store.folderSubtree(folder.id));
      else store.toggleFolder(folder.id);
    }}><Icon name="chevron-right" size={12} stroke={2.4} /></button
  >
  <span class="row-icon"><Icon name={folder.expanded ? 'folder-open' : 'folder'} size={15} /></span>
  <span class="row-label">{folder.title}</span>
  {#if unread > 0}<span class="row-count">{countLabel(unread)}</span>{/if}
</div>

{#if folder.expanded}
  {#each node.children as child (childKey(child))}
    {#if child.kind === 'folder'}
      <TreeFolder node={child.node} depth={depth + 1} />
    {:else}
      <FeedRow feed={child.feed} depth={depth + 1} />
    {/if}
  {/each}
{/if}
