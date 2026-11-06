<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import ListHeader from './ListHeader.svelte';
  import ArticleRow from './ArticleRow.svelte';
  import { store } from '../lib/store.svelte';
  import { dayGroup } from '../lib/util';

  let scroller: HTMLDivElement | undefined = $state();

  // Back to the top whenever a new list is loaded.
  $effect(() => {
    store.listVersion;
    if (scroller) scroller.scrollTop = 0;
  });

  // Keep the selected row visible during keyboard navigation.
  $effect(() => {
    const id = store.selectedId;
    if (id == null || !scroller) return;
    const row = scroller.querySelector<HTMLElement>(`[data-id="${id}"]`);
    row?.scrollIntoView({ block: 'nearest' });
  });

  function onscroll() {
    if (!scroller) return;
    if (scroller.scrollTop + scroller.clientHeight > scroller.scrollHeight - 600) store.loadMore();
  }

  const groups = $derived.by(() => {
    const now = store.now;
    let last = '';
    return store.articles.map((a) => {
      const label = store.oldestFirst ? '' : dayGroup(a.published, now);
      const show = label !== last;
      last = label;
      return { article: a, header: show ? label : '' };
    });
  });

  const emptyText = $derived.by(() => {
    if (store.query) return { title: 'No matches', body: `Nothing matches “${store.query}”.` };
    if (store.filter === 'unread') return { title: 'All caught up', body: 'There are no unread articles here.' };
    if (store.filter === 'starred' || store.scope.kind === 'starred')
      return { title: 'No starred articles', body: 'Press S on an article to keep it here. Starred articles are never archived.' };
    if (store.filter === 'new') return { title: 'Nothing new', body: 'New articles from this session appear here.' };
    return { title: 'No articles yet', body: 'Articles appear here after the feed refreshes.' };
  });
</script>

<section class="list-pane">
  <ListHeader />
  <div class="list-scroll" bind:this={scroller} {onscroll} role="listbox" tabindex="-1">
    {#each groups as { article, header } (article.id)}
      {#if header}<div class="day">{header}</div>{/if}
      <ArticleRow {article} />
    {/each}

    {#if store.listLoading && store.articles.length === 0}
      <div class="loading"><span class="spinner"></span></div>
    {:else if store.articles.length === 0}
      <div class="empty">
        <div class="empty-icon">
          <Icon name={store.filter === 'unread' ? 'check' : store.query ? 'search' : 'newspaper'} size={24} stroke={1.6} />
        </div>
        <h3>{emptyText.title}</h3>
        <p>{emptyText.body}</p>
        {#if store.filter !== 'all' && !store.query}
          <button class="btn small" onclick={() => store.setFilter('all')}>Show all articles</button>
        {/if}
      </div>
    {:else if store.hasMore}
      <div class="loading"><span class="spinner"></span></div>
    {/if}
  </div>
</section>

<style>
  .list-pane {
    display: grid;
    grid-template-rows: auto 1fr;
    min-width: 0;
    min-height: 0;
    background: var(--bg-list);
    border-right: 1px solid var(--border);
  }
  :global(.reading.normal) .list-pane {
    border-right: none;
    border-bottom: 1px solid var(--border);
  }
  .list-scroll {
    overflow-y: auto;
    overflow-x: hidden;
    min-height: 0;
    padding: 4px 0 24px;
  }
  .day {
    position: sticky;
    top: 0;
    z-index: 2;
    padding: 10px 18px 6px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-3);
    background: linear-gradient(var(--bg-list) 70%, transparent);
  }
  .loading {
    display: grid;
    place-items: center;
    padding: 24px;
  }
  .empty {
    display: grid;
    justify-items: center;
    gap: 6px;
    padding: 64px 32px;
    text-align: center;
    color: var(--text-3);
    animation: fade-up 0.3s var(--ease);
  }
  .empty-icon {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    border-radius: 16px;
    background: var(--bg-muted);
    color: var(--text-3);
    margin-bottom: 6px;
  }
  .empty h3 {
    margin: 0;
    font-size: 14px;
    color: var(--text);
  }
  .empty p {
    margin: 0 0 10px;
    font-size: 12.5px;
    max-width: 260px;
    line-height: 1.5;
  }
</style>
