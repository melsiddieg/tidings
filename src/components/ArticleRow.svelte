<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { store } from '../lib/store.svelte';
  import type { ArticleItem } from '../lib/types';
  import { copyText, shortTime } from '../lib/util';

  let { article }: { article: ArticleItem } = $props();

  const feed = $derived(store.feedById.get(article.feedId));
  const selected = $derived(store.selectedId === article.id);
  const read = $derived(store.isRead(article));
  const starred = $derived(store.isStarred(article));
  let thumbFailed = $state(false);

  function menu(e: MouseEvent) {
    store.selectedId = article.id;
    store.openMenu(e, [
      { label: 'Open in new tab', icon: 'book', shortcut: 'O', action: () => store.openArticleTab(article.id) },
      { label: 'Open in browser', icon: 'globe', shortcut: 'V', disabled: !article.url, action: () => store.openExternal(article.url) },
      { label: 'Copy link', icon: 'link', disabled: !article.url, action: () => article.url && copyText(article.url).then(() => store.toast('Link copied')) },
      { separator: true },
      { label: read ? 'Mark as unread' : 'Mark as read', icon: read ? 'circle' : 'check', shortcut: 'M', action: () => store.markRead([article.id], !read) },
      { label: starred ? 'Remove star' : 'Star', icon: 'star', shortcut: 'S', action: () => store.setStarred(article.id, !starred) },
      { separator: true },
      { label: 'Delete article', icon: 'trash', danger: true, action: () => store.deleteArticle(article.id) },
    ]);
  }
</script>

<div
  class="row"
  class:selected
  class:read
  data-id={article.id}
  role="option"
  aria-selected={selected}
  tabindex="-1"
  onclick={() => store.selectArticle(article.id)}
  ondblclick={() => store.openArticleTab(article.id)}
  onauxclick={(e) => {
    if (e.button === 1) {
      e.preventDefault();
      store.openArticleTab(article.id, true);
    }
  }}
  onkeydown={() => {}}
  oncontextmenu={menu}
>
  <span class="marker" class:on={!read}></span>
  <div class="body">
    <div class="meta">
      <FeedIcon {feed} size={14} />
      <span class="feed">{feed?.title ?? ''}</span>
      {#if article.hasEnclosure}<span class="clip" title="Has attachment"><Icon name="paperclip" size={11} /></span>{/if}
      <span class="time" title={new Date(article.published * 1000).toLocaleString()}>{shortTime(article.published, store.now)}</span>
    </div>
    <div class="title" dir="auto">{article.title}</div>
    {#if article.excerpt}
      <div class="excerpt" dir="auto">{article.excerpt}</div>
    {/if}
  </div>
  {#if article.thumb && !thumbFailed}
    <img class="thumb" src={article.thumb} alt="" loading="lazy" decoding="async" onerror={() => (thumbFailed = true)} />
  {/if}
  {#if starred}
    <span class="star"><Icon name="star" size={13} fill /></span>
  {/if}
</div>

<style>
  .row {
    position: relative;
    display: flex;
    gap: 12px;
    margin: 1px 8px;
    padding: 10px 12px 11px 16px;
    border-radius: 10px;
    transition: background 0.12s var(--ease);
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.selected {
    background: var(--accent-soft);
  }
  .marker {
    position: absolute;
    left: 6px;
    top: 17px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: transparent;
    transition: background 0.2s var(--ease), transform 0.2s var(--ease);
    transform: scale(0.4);
  }
  .marker.on {
    background: var(--accent);
    transform: scale(1);
  }
  .body {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 3px;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--text-3);
    min-width: 0;
  }
  .feed {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-weight: 500;
    color: var(--text-2);
  }
  .clip {
    display: grid;
  }
  .time {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .title {
    font-size: 14px;
    font-weight: 650;
    line-height: 1.35;
    letter-spacing: -0.008em;
    color: var(--text);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .excerpt {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--text-2);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .row.read .title {
    font-weight: 500;
    color: var(--text-2);
  }
  .row.read .excerpt {
    color: var(--text-3);
  }
  .row.read.selected .title {
    color: var(--text);
  }
  .thumb {
    flex: none;
    width: 64px;
    height: 64px;
    margin-top: 2px;
    border-radius: 8px;
    object-fit: cover;
    background: var(--bg-muted);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .row.read .thumb {
    opacity: 0.75;
  }
  .star {
    position: absolute;
    right: 8px;
    bottom: 8px;
    color: #f5a524;
  }
</style>
