<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { drag, endDrag, startDrag, zoneFor, zoneOf } from '../lib/dnd.svelte';
  import { store } from '../lib/store.svelte';
  import type { Feed, ItemRef } from '../lib/types';
  import { countLabel, sameItem } from '../lib/util';

  let { feed, depth }: { feed: Feed; depth: number } = $props();

  const selected = $derived(store.scope.kind === 'feed' && store.scope.id === feed.id);
  const ref = $derived<ItemRef>({ kind: 'feed', id: feed.id });
  const key = $derived(`feed:${feed.id}`);
  const zone = $derived(zoneFor(key));
  const pinned = $derived(feed.pinned > 0);
  const refreshing = $derived(!!store.progress);

  function menu(e: MouseEvent) {
    store.openMenu(e, [
      { label: 'Refresh', icon: 'refresh', action: () => store.refresh({ feedIds: [feed.id] }) },
      { label: 'Mark all as read', icon: 'check-check', action: () => store.markAllRead({ kind: 'feed', id: feed.id }) },
      { separator: true },
      { label: 'Open website', icon: 'globe', disabled: !feed.siteUrl, action: () => store.openExternal(feed.siteUrl) },
      { label: pinned ? 'Remove from Favorites' : 'Add to Favorites', icon: 'star', action: () => store.setPinned(ref, !pinned) },
      { label: 'Copy feed address', icon: 'copy', action: () => navigator.clipboard?.writeText(feed.url).then(() => store.toast('Feed address copied')) },
      { separator: true },
      { label: 'Properties…', icon: 'pencil', action: () => (store.dialog = { kind: 'feed', feedId: feed.id }) },
      { label: 'Unsubscribe…', icon: 'trash', danger: true, action: () => store.deleteFeed(feed) },
    ]);
  }

  // Top half: drop before this feed. Bottom half: after it.
  function ondragover(e: DragEvent) {
    const item = drag.item;
    if (!item || sameItem(item, ref)) return;
    e.preventDefault();
    e.stopPropagation();
    drag.over = `${key}:${zoneOf(e, false)}`;
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    const item = drag.item;
    const z = zoneOf(e, false);
    endDrag();
    if (!item || sameItem(item, ref)) return;
    const before = z === 'before' ? ref : store.itemAfter(feed.folderId, ref, item);
    store.moveItem(item, feed.folderId, before);
  }
</script>

<div
  class="tree-row feed"
  class:selected
  class:paused={feed.paused}
  class:drop-before={zone === 'before'}
  class:drop-after={zone === 'after'}
  style:--depth={depth}
  role="treeitem"
  aria-selected={selected}
  tabindex="-1"
  draggable="true"
  title={feed.lastError ? `${feed.title}\n${feed.lastError}` : feed.title}
  ondragstart={(e) => startDrag(e, ref)}
  ondragend={endDrag}
  {ondragover}
  ondragleave={() => zone && (drag.over = null)}
  {ondrop}
  onclick={() => store.selectScope({ kind: 'feed', id: feed.id })}
  onkeydown={() => {}}
  oncontextmenu={menu}
>
  <span class="twisty-space"></span>
  <span class="row-icon"><FeedIcon {feed} size={16} /></span>
  <span class="row-label">{feed.title}</span>
  {#if feed.lastError && !refreshing}
    <span class="row-warn" title={feed.lastError}><Icon name="alert" size={13} /></span>
  {:else if feed.paused}
    <span class="row-warn muted" title="Updates paused"><Icon name="pause" size={12} /></span>
  {/if}
  {#if feed.unread > 0}<span class="row-count">{countLabel(feed.unread)}</span>{/if}
</div>
