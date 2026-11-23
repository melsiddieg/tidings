<script lang="ts">
  import { rangeLabel } from '../lib/filters';
  import Icon from '../lib/Icon.svelte';
  import ListHeader from './ListHeader.svelte';
  import ArticleRow from './ArticleRow.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { countLabel } from '../lib/util';
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

  // Feeds that match the search, shown above the articles with the folder each is in.
  const SHOWN_FEEDS = 5;
  let allFeeds = $state(false);
  $effect(() => {
    store.query;
    allFeeds = false;
  });
  const feedMatches = $derived(allFeeds ? store.feedMatches : store.feedMatches.slice(0, SHOWN_FEEDS));

  const emptyText = $derived.by(() => {
    if (store.query && store.feedMatches.length) return { title: 'No articles match', body: `No articles match “${store.query}”.` };
    if (store.query) return { title: 'No matches', body: `Nothing matches “${store.query}”.` };
    if (store.dateFiltered)
      return { title: 'Nothing in this period', body: `No articles here for “${rangeLabel(store.dateRange)}”. Pick another date range above.` };
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
    {#if store.feedMatches.length}
      <div class="day">Feeds</div>
      <div class="matches">
        {#each feedMatches as f (f.id)}
          {@const path = store.folderPath(f.folderId)}
          <div class="match" role="button" tabindex="-1" title="Show this feed" onclick={() => store.showFeed(f)} onkeydown={() => {}}>
            <FeedIcon feed={f} size={16} />
            <span class="m-text">
              <span class="m-title">{f.title}</span>
              <span class="m-path"><Icon name="folder" size={11} />{path || 'No folder'}</span>
            </span>
            {#if f.unread > 0}<span class="m-count">{countLabel(f.unread)}</span>{/if}
            <button
              class="btn small"
              title="Move this feed to another folder"
              onclick={(e) => {
                e.stopPropagation();
                store.openMenuAt(e.currentTarget as HTMLElement, store.folderMenuItems(f));
              }}><Icon name="folder" size={12} />Move</button
            >
          </div>
        {/each}
        {#if !allFeeds && store.feedMatches.length > SHOWN_FEEDS}
          <button class="more" onclick={() => (allFeeds = true)}>Show {store.feedMatches.length - SHOWN_FEEDS} more feeds</button>
        {/if}
      </div>
      {#if store.articles.length}<div class="day">Articles</div>{/if}
    {/if}
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
  .matches {
    display: grid;
    gap: 1px;
    padding-bottom: 6px;
  }
  .match {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 8px;
    padding: 7px 10px 7px 16px;
    border-radius: 10px;
    cursor: default;
  }
  .match:hover {
    background: var(--bg-hover);
  }
  .m-text {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 1px;
  }
  .m-title,
  .m-path {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .m-title {
    font-size: 13px;
    font-weight: 600;
  }
  .m-path {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .m-path :global(svg) {
    flex: none;
  }
  .m-count {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .more {
    justify-self: start;
    margin: 2px 0 0 24px;
    padding: 2px 4px;
    border: none;
    background: none;
    color: var(--accent);
    font-size: 12px;
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
