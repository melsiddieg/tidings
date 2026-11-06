<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { drag, endDrag, startDrag } from '../lib/dnd.svelte';
  import { store } from '../lib/store.svelte';
  import type { Feed } from '../lib/types';
  import { countLabel } from '../lib/util';

  let { feed, depth }: { feed: Feed; depth: number } = $props();

  const selected = $derived(store.scope.kind === 'feed' && store.scope.id === feed.id);
  const dropKey = $derived(`feed:${feed.id}`);
  const refreshing = $derived(!!store.progress);

  function menu(e: MouseEvent) {
    store.openMenu(e, [
      { label: 'Refresh', icon: 'refresh', action: () => store.refresh({ feedIds: [feed.id] }) },
      { label: 'Mark all as read', icon: 'check-check', action: () => store.markAllRead({ kind: 'feed', id: feed.id }) },
      { separator: true },
      { label: 'Open website', icon: 'globe', disabled: !feed.siteUrl, action: () => store.openExternal(feed.siteUrl) },
      { label: 'Copy feed address', icon: 'copy', action: () => navigator.clipboard?.writeText(feed.url).then(() => store.toast('Feed address copied')) },
      { separator: true },
      { label: 'Properties…', icon: 'pencil', action: () => (store.dialog = { kind: 'feed', feedId: feed.id }) },
      { label: 'Unsubscribe…', icon: 'trash', danger: true, action: () => store.deleteFeed(feed) },
    ]);
  }

  function ondragover(e: DragEvent) {
    const item = drag.item;
    if (!item || (item.type === 'feed' && item.id === feed.id)) return;
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
    if (item.type === 'feed') store.moveFeed(item.id, feed.folderId, feed.id);
    else store.moveFolder(item.id, feed.folderId, null);
  }
</script>

<div
  class="tree-row feed"
  class:selected
  class:paused={feed.paused}
  class:drop-before={drag.over === dropKey && drag.item?.type === 'feed'}
  class:drop-into={drag.over === dropKey && drag.item?.type === 'folder'}
  style:--depth={depth}
  role="treeitem"
  aria-selected={selected}
  tabindex="-1"
  draggable="true"
  title={feed.lastError ? `${feed.title}\n${feed.lastError}` : feed.title}
  ondragstart={(e) => startDrag(e, { type: 'feed', id: feed.id })}
  ondragend={endDrag}
  {ondragover}
  ondragleave={() => drag.over === dropKey && (drag.over = null)}
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
