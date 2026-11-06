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
  const zone = $derived(drag.over?.startsWith(`${dropKey}:`) ? drag.over.slice(dropKey.length + 1) : null);

  type Zone = 'before' | 'into' | 'after';

  /** The sibling that follows this folder once `movingId` is taken out of the list. */
  function folderAfter(movingId: number): number | null {
    const siblings = store.tree.folders
      .filter((f) => f.parentId === folder.parentId && f.id !== movingId)
      .sort((a, b) => a.position - b.position || a.title.localeCompare(b.title));
    const index = siblings.findIndex((f) => f.id === folder.id);
    return siblings[index + 1]?.id ?? null;
  }

  /** Where a dragged item would land: above this folder, inside it, or below it. */
  function zoneFor(e: DragEvent): Zone {
    // Feeds always go inside a folder; feeds and folders are separate lists.
    if (drag.item?.type !== 'folder') return 'into';
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const y = (e.clientY - rect.top) / rect.height;
    if (y < 0.28) return 'before';
    // An open folder's children follow its row, so "below" would be ambiguous there.
    if (y > 0.72 && !folder.expanded) return 'after';
    return 'into';
  }

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
    drag.over = `${dropKey}:${zoneFor(e)}`;
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    const item = drag.item;
    const where = zoneFor(e);
    endDrag();
    if (!item) return;
    if (item.type === 'feed') {
      store.moveFeed(item.id, folder.id, null);
    } else if (item.id !== folder.id) {
      if (where === 'before') store.moveFolder(item.id, folder.parentId, folder.id);
      else if (where === 'after') store.moveFolder(item.id, folder.parentId, folderAfter(item.id));
      else store.moveFolder(item.id, folder.id, null);
    }
  }
</script>

<div
  class="tree-row"
  class:selected
  class:drop-into={zone === 'into'}
  class:drop-before={zone === 'before'}
  class:drop-after={zone === 'after'}
  style:--depth={depth}
  role="treeitem"
  aria-selected={selected}
  aria-expanded={folder.expanded}
  tabindex="-1"
  draggable="true"
  ondragstart={(e) => startDrag(e, { type: 'folder', id: folder.id })}
  ondragend={endDrag}
  {ondragover}
  ondragleave={() => zone && (drag.over = null)}
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
