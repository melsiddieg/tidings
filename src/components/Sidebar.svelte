<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { drag, endDrag } from '../lib/dnd.svelte';
  import { store } from '../lib/store.svelte';
  import { childKey, countLabel, relativeAgo } from '../lib/util';
  import FeedIcon from './FeedIcon.svelte';
  import type { FeedType, ItemRef } from '../lib/types';
  import TreeFolder from './TreeFolder.svelte';
  import FeedRow from './FeedRow.svelte';

  const allSelected = $derived(store.scope.kind === 'all');
  const starredSelected = $derived(store.scope.kind === 'starred');

  function moreMenu(e: MouseEvent) {
    store.openMenuAt(e.currentTarget as HTMLElement, [
      { label: 'Add feed…', icon: 'plus', shortcut: '⌘N', action: () => (store.dialog = { kind: 'add-feed', folderId: null }) },
      { label: 'New folder…', icon: 'folder-plus', action: () => (store.dialog = { kind: 'folder', folderId: null, parentId: null }) },
      { separator: true },
      { label: 'Import OPML…', icon: 'download', action: () => store.importOpml() },
      { label: 'Export OPML…', icon: 'upload', action: () => store.exportOpml() },
      { separator: true },
      { label: 'Refresh all feeds', icon: 'refresh', action: () => store.refresh() },
      { label: 'Mark everything as read', icon: 'check-check', action: () => store.markAllRead({ kind: 'all' }) },
      { separator: true },
      { label: 'Keyboard shortcuts', icon: 'keyboard', action: () => (store.dialog = { kind: 'shortcuts' }) },
      { label: 'Settings…', icon: 'settings', action: () => (store.dialog = { kind: 'settings' }) },
    ]);
  }

  function favoriteMenu(e: MouseEvent, ref: ItemRef) {
    store.openMenu(e, [
      { label: 'Refresh', icon: 'refresh', action: () => (ref.kind === 'feed' ? store.refresh({ feedIds: [ref.id] }) : store.refresh({ folderId: ref.id })) },
      { label: 'Mark all as read', icon: 'check-check', action: () => store.markAllRead(ref.kind === 'feed' ? { kind: 'feed', id: ref.id } : { kind: 'folder', id: ref.id }) },
      { separator: true },
      { label: 'Remove from Favorites', icon: 'star', action: () => store.setPinned(ref, false) },
    ]);
  }

  function typeMenu(e: MouseEvent, type: FeedType) {
    store.openMenu(e, [
      { label: 'Refresh', icon: 'refresh', action: () => store.refresh({ feedIds: store.feedsOfType(type).map((f) => f.id) }) },
      { label: 'Mark all as read', icon: 'check-check', action: () => store.markAllRead({ kind: 'type', type }) },
    ]);
  }

  // Dropping on the empty part of the list moves an item to the top level.
  function onRootDragOver(e: DragEvent) {
    if (!drag.item) return;
    e.preventDefault();
    drag.over = 'root';
  }

  function onRootDrop(e: DragEvent) {
    e.preventDefault();
    const item = drag.item;
    endDrag();
    if (!item) return;
    store.moveItem(item, null, null);
  }

  const status = $derived.by(() => {
    if (store.progress) return `Updating ${store.progress.done} of ${store.progress.total}…`;
    if (store.lastRefresh) return `Updated ${relativeAgo(store.lastRefresh, store.now)}`;
    return 'Refresh all';
  });
</script>

<aside class="sidebar">
  <div class="head">
    <div class="brand">
      <span class="logo"><Icon name="rss" size={13} stroke={2.6} /></span>
      <span>Tidings</span>
    </div>
    <div class="head-actions">
      <button class="icon-btn" title="Add feed" onclick={() => (store.dialog = { kind: 'add-feed', folderId: null })}>
        <Icon name="plus" size={17} />
      </button>
      <button class="icon-btn" title="More" onclick={moreMenu}><Icon name="more" size={17} /></button>
    </div>
  </div>

  <nav class="scroll" ondragover={onRootDragOver} ondrop={onRootDrop} ondragleave={() => (drag.over = null)} class:drop-root={drag.over === 'root'}>
    <div class="group">
      <button class="item" class:selected={allSelected} onclick={() => store.selectScope({ kind: 'all' })}>
        <Icon name="newspaper" size={16} />
        <span class="label">All articles</span>
        {#if store.unreadTotal > 0}<span class="count">{countLabel(store.unreadTotal)}</span>{/if}
      </button>
      <button class="item" class:selected={starredSelected} onclick={() => store.selectScope({ kind: 'starred' })}>
        <Icon name="star" size={16} />
        <span class="label">Starred</span>
        {#if store.tree.starred > 0}<span class="count muted">{countLabel(store.tree.starred)}</span>{/if}
      </button>
    </div>

    {#if store.favorites.length > 0}
      <div class="section-title"><span>Favorites</span></div>
      <div class="group">
        {#each store.favorites as fav (`${fav.ref.kind}:${fav.ref.id}`)}
          {@const isFeed = fav.ref.kind === 'feed'}
          {@const unread = isFeed ? (store.feedById.get(fav.ref.id)?.unread ?? 0) : (store.folderUnread.get(fav.ref.id) ?? 0)}
          {@const selected = store.scope.kind === fav.ref.kind && 'id' in store.scope && store.scope.id === fav.ref.id}
          <button
            class="item"
            class:selected
            onclick={() => store.selectScope(fav.ref.kind === 'feed' ? { kind: 'feed', id: fav.ref.id } : { kind: 'folder', id: fav.ref.id })}
            oncontextmenu={(e) => favoriteMenu(e, fav.ref)}
          >
            {#if isFeed}
              <FeedIcon feed={store.feedById.get(fav.ref.id)} size={16} />
            {:else}
              <Icon name="folder" size={16} />
            {/if}
            <span class="label">{fav.title}</span>
            {#if unread > 0}<span class="count">{countLabel(unread)}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}

    {#if store.typeSummary.length > 1}
      <div class="section-title"><span>Types</span></div>
      <div class="group">
        {#each store.typeSummary as row (row.info.value)}
          <button
            class="item"
            class:selected={store.scope.kind === 'type' && store.scope.type === row.info.value}
            title={`${row.feeds} ${row.feeds === 1 ? 'feed' : 'feeds'}`}
            onclick={() => store.selectScope({ kind: 'type', type: row.info.value })}
            oncontextmenu={(e) => typeMenu(e, row.info.value)}
          >
            <Icon name={row.info.icon} size={16} />
            <span class="label">{row.info.label}</span>
            {#if row.unread > 0}<span class="count">{countLabel(row.unread)}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}

    <div class="section-title">
      <span>Subscriptions</span>
      <button class="icon-btn mini" title="New folder" onclick={() => (store.dialog = { kind: 'folder', folderId: null, parentId: null })}>
        <Icon name="folder-plus" size={14} />
      </button>
    </div>

    <div class="tree" role="tree">
      {#each store.root.children as child (childKey(child))}
        {#if child.kind === 'folder'}
          <TreeFolder node={child.node} depth={0} />
        {:else}
          <FeedRow feed={child.feed} depth={0} />
        {/if}
      {/each}
    </div>

    {#if store.ready && store.tree.feeds.length === 0 && store.tree.folders.length === 0}
      <p class="empty">No subscriptions yet.</p>
    {/if}
  </nav>

  <footer class="foot">
    <button class="refresh" onclick={() => store.refresh()} disabled={!!store.progress} title="Refresh all feeds">
      <span class="refresh-icon" class:spinning={!!store.progress}><Icon name="refresh" size={14} /></span>
      <span>{status}</span>
    </button>
    <button class="icon-btn" title="Settings" onclick={() => (store.dialog = { kind: 'settings' })}>
      <Icon name="settings" size={16} />
    </button>
  </footer>
</aside>

<style>
  .sidebar {
    display: grid;
    grid-template-rows: auto 1fr auto;
    min-width: 0;
    min-height: 0;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 10px 6px 16px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    font-size: 14px;
    letter-spacing: -0.01em;
  }
  .logo {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 7px;
    color: #fff;
    background: linear-gradient(145deg, #ffa061, #f4683c 55%, #d2402e);
    box-shadow: 0 1px 3px rgba(210, 64, 46, 0.4), inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }
  .head-actions {
    display: flex;
    gap: 2px;
  }
  .scroll {
    overflow-y: auto;
    overflow-x: hidden;
    padding: 6px 0 16px;
    min-height: 0;
  }
  .scroll.drop-root {
    background: var(--accent-softer);
  }
  .group {
    display: grid;
    gap: 1px;
    padding: 0 8px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    text-align: left;
    font-size: 13px;
    font-weight: 500;
    transition: background 0.12s var(--ease);
  }
  .item :global(.icon) {
    color: var(--text-2);
  }
  .item:hover {
    background: var(--bg-hover);
  }
  .item.selected {
    background: var(--accent-soft);
  }
  .item.selected :global(.icon) {
    color: var(--accent);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .count {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .item.selected .count {
    color: var(--accent);
  }
  .count.muted {
    font-weight: 500;
    color: var(--text-3);
  }
  .section-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 18px 10px 4px 18px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .icon-btn.mini {
    width: 22px;
    height: 22px;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .section-title:hover .icon-btn.mini {
    opacity: 1;
  }
  .tree {
    display: grid;
    gap: 1px;
  }
  .empty {
    margin: 8px 18px;
    color: var(--text-3);
    font-size: 12px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 8px 10px 8px;
    border-top: 1px solid var(--border);
  }
  .refresh {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text-3);
    font-size: 12px;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
  }
  .refresh span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .refresh:hover {
    background: var(--bg-hover);
    color: var(--text-2);
  }
  .refresh:disabled {
    opacity: 1;
  }
  .refresh-icon {
    display: grid;
  }
  .spinning {
    animation: spin 1s linear infinite;
    color: var(--accent);
  }
</style>
