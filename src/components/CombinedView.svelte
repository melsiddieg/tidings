<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import ListHeader from './ListHeader.svelte';
  import ArticleBody from './ArticleBody.svelte';
  import { api } from '../lib/api';
  import { player, isAudio } from '../lib/player.svelte';
  import { store } from '../lib/store.svelte';
  import type { ArticleDetail } from '../lib/types';
  import { longTime, videoSource } from '../lib/util';

  const PAGE = 15;
  let items = $state<ArticleDetail[]>([]);
  let loading = $state(false);
  let done = $state(false);
  let scroller: HTMLDivElement | undefined = $state();
  let token = 0;

  async function load(reset: boolean) {
    if (loading && !reset) return;
    const t = reset ? ++token : token;
    loading = true;
    try {
      const offset = reset ? 0 : items.length;
      const batch = await api.listArticlesFull(store.listQuery(offset, PAGE));
      if (t !== token) return;
      items = reset ? batch : [...items, ...batch];
      done = batch.length < PAGE;
      if (reset && scroller) scroller.scrollTop = 0;
    } catch (e) {
      store.error(e);
    } finally {
      if (t === token) loading = false;
    }
  }

  $effect(() => {
    store.listVersion;
    untrack(() => load(true));
  });

  // Follow keyboard selection (J/K) through the cards.
  $effect(() => {
    const id = store.selectedId;
    if (id == null || !scroller) return;
    scroller.querySelector<HTMLElement>(`[data-id="${id}"]`)?.scrollIntoView({ block: 'start', behavior: 'smooth' });
  });

  function onscroll() {
    if (!scroller || done) return;
    if (scroller.scrollTop + scroller.clientHeight > scroller.scrollHeight - 1200) load(false);
  }

  let lastSeq = store.command.seq;
  $effect(() => {
    const cmd = store.command;
    if (cmd.seq === lastSeq) return;
    lastSeq = cmd.seq;
    const id = store.selectedId;
    if (store.activeTabId !== 'main' || id == null) return;
    const item = items.find((a) => a.id === id);
    if (!item) return;
    untrack(() => {
      if (cmd.name === 'star') store.setStarred(id, !store.isStarred(item));
      else if (cmd.name === 'read') store.markRead([id], !store.isRead(item));
      else if (cmd.name === 'browser') store.openExternal(item.url);
      else if (cmd.name === 'delete') store.deleteArticle(id);
    });
  });
</script>

<section class="combined">
  <div class="combined-head"><ListHeader /></div>
  <div class="combined-scroll article-scroll" bind:this={scroller} {onscroll}>
    {#each items as item (item.id)}
      {@const feed = store.feedById.get(item.feedId)}
      {@const read = store.isRead(item)}
      {@const starred = store.isStarred(item)}
      <article class="card" class:selected={store.selectedId === item.id} class:read data-id={item.id}>
        <header class="article-head">
          <div class="kicker">
            <FeedIcon {feed} size={16} />
            <button class="feed-link" onclick={() => store.selectScope({ kind: 'feed', id: item.feedId })}>{feed?.title}</button>
            <span class="dot-sep">·</span>
            <time>{longTime(item.published)}</time>
          </div>
          <h2 class="article-title small" dir="auto">
            <button class="title-btn" onclick={() => { store.selectArticle(item.id); store.openArticleTab(item.id); }}>{item.title}</button>
          </h2>
          {#if item.author}<div class="byline">By {item.author}</div>{/if}
        </header>
        <ArticleBody html={item.html} />
        <footer class="card-foot">
          <button class="icon-btn" class:active={starred} title="Star" onclick={() => store.setStarred(item.id, !starred)}>
            <Icon name="star" size={15} fill={starred} />
          </button>
          <button class="btn small ghost" onclick={() => store.markRead([item.id], !read)}>
            <Icon name={read ? 'circle' : 'check'} size={13} />{read ? 'Mark unread' : 'Mark read'}
          </button>
          {#if item.enclosureUrl && isAudio(item.enclosureType, item.enclosureUrl)}
            {@const current = player.isCurrent(item.enclosureUrl)}
            <button class="btn small" onclick={() => item.enclosureUrl && player.play({ articleId: item.id, feedId: item.feedId, title: item.title, url: item.enclosureUrl, type: item.enclosureType })}>
              <Icon name={current && player.playing ? 'pause-fill' : 'play'} size={12} stroke={0} />{current && player.playing ? 'Pause' : 'Play episode'}
            </button>
          {/if}
          {#if videoSource(item.url, item.enclosureUrl, item.enclosureType)}
            {@const v = videoSource(item.url, item.enclosureUrl, item.enclosureType)?.url}
            <button class="btn small" onclick={() => store.watchVideo(v)}>
              <Icon name="play" size={12} stroke={0} />Watch
            </button>
            {#if videoSource(item.url, item.enclosureUrl, item.enclosureType)?.kind === 'site' && v}
              {@const current = player.isCurrent(v)}
              <button class="btn small ghost" onclick={() => player.listen(item.id, item.feedId, item.title, v)}>
                <Icon name={current && player.playing ? 'pause-fill' : 'headphones'} size={12} stroke={current && player.playing ? 0 : undefined} />{current && player.playing ? 'Pause' : 'Listen'}
              </button>
            {/if}
          {/if}
          <span class="grow"></span>
          <button class="btn small ghost" onclick={() => store.openArticleTab(item.id)}><Icon name="book" size={13} />Open in tab</button>
          {#if item.url}
            <button class="btn small ghost" onclick={() => store.openExternal(item.url)}><Icon name="globe" size={13} />Browser</button>
          {/if}
        </footer>
      </article>
    {/each}
    {#if loading}
      <div class="article-state small"><span class="spinner"></span></div>
    {:else if items.length === 0}
      <div class="article-state">
        <Icon name="newspaper" size={26} stroke={1.5} />
        <p>No articles to show.</p>
      </div>
    {/if}
  </div>
</section>

<style>
  .combined {
    display: grid;
    grid-template-rows: auto 1fr;
    min-width: 0;
    min-height: 0;
    background: var(--bg-content);
  }
  .combined-head {
    background: var(--bg-list);
  }
  .combined-head :global(.list-head) {
    max-width: 100%;
  }
  .combined-scroll {
    padding: 8px 0 80px;
  }
  .card {
    max-width: 760px;
    margin: 0 auto;
    padding: 44px 44px 24px;
    border-bottom: 1px solid var(--border);
    scroll-margin-top: 8px;
  }
  .card.selected {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .card.read .article-title {
    color: var(--text-2);
  }
  .article-title.small {
    font-size: calc(var(--reader-size) * 1.4);
  }
  .title-btn {
    all: unset;
    cursor: pointer;
  }
  .title-btn:hover {
    color: var(--accent);
  }
  .card-foot {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 20px;
  }
  .grow {
    flex: 1;
  }
  .article-state.small {
    height: auto;
    padding: 24px;
  }
</style>
