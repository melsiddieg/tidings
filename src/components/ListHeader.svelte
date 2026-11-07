<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { store } from '../lib/store.svelte';
  import type { Filter, Settings } from '../lib/types';
  import { DATE_PRESETS, rangeLabel, typeInfo } from '../lib/filters';

  let searchInput: HTMLInputElement | undefined = $state();
  let draft = $state(store.query);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const FILTERS: { value: Filter; label: string; icon: string }[] = [
    { value: 'all', label: 'All', icon: 'newspaper' },
    { value: 'unread', label: 'Unread', icon: 'dot' },
    { value: 'new', label: 'New', icon: 'sparkles' },
    { value: 'starred', label: 'Starred', icon: 'star' },
  ];

  const filterLabel = $derived(FILTERS.find((f) => f.value === store.filter)?.label ?? 'All');
  const feed = $derived(store.scope.kind === 'feed' ? store.feedById.get(store.scope.id) : undefined);
  const scopeIcon = $derived.by(() => {
    const s = store.scope;
    if (s.kind === 'all') return 'newspaper';
    if (s.kind === 'starred') return 'star';
    if (s.kind === 'type') return typeInfo(s.type).icon;
    return 'folder';
  });
  const dateLabel = $derived(rangeLabel(store.dateRange));

  // Ctrl/Cmd+F and "/" focus the search box.
  $effect(() => {
    if (store.searchFocus > 0) {
      searchInput?.focus();
      searchInput?.select();
    }
  });

  function oninput() {
    clearTimeout(timer);
    timer = setTimeout(() => store.setQuery(draft), 220);
  }

  function clear() {
    draft = '';
    store.setQuery('');
    searchInput?.focus();
  }

  function filterMenu(e: MouseEvent) {
    store.openMenuAt(
      e.currentTarget as HTMLElement,
      FILTERS.map((f) => ({
        label: f.label,
        icon: f.icon,
        checked: store.filter === f.value,
        action: () => store.setFilter(f.value),
      })),
    );
  }

  function dateMenu(e: MouseEvent) {
    const preset = store.dateRange.preset;
    store.openMenuAt(e.currentTarget as HTMLElement, [
      ...DATE_PRESETS.map((p) => ({
        label: p.label,
        checked: preset === p.value,
        action: () => store.setDateRange({ preset: p.value }),
      })),
      { separator: true },
      { label: 'Custom range…', icon: 'calendar', checked: preset === 'custom', action: () => (store.dialog = { kind: 'date-range' }) },
    ]);
  }

  function viewMenu(e: MouseEvent) {
    const views: { value: Settings['viewMode']; label: string }[] = [
      { value: 'widescreen', label: 'Widescreen' },
      { value: 'normal', label: 'Normal' },
      { value: 'combined', label: 'Combined' },
    ];
    store.openMenuAt(e.currentTarget as HTMLElement, [
      ...views.map((v) => ({
        label: v.label,
        icon: v.value,
        checked: store.settings.viewMode === v.value,
        action: () => store.updateSettings({ viewMode: v.value }),
      })),
      { separator: true },
      { label: 'Newest first', checked: !store.oldestFirst, action: () => store.setOldestFirst(false) },
      { label: 'Oldest first', checked: store.oldestFirst, action: () => store.setOldestFirst(true) },
    ]);
  }
</script>

<header class="list-head">
  <div class="title-row">
    <div class="scope">
      {#if feed}
        <FeedIcon {feed} size={18} />
      {:else}
        <span class="scope-icon"><Icon name={scopeIcon} size={16} /></span>
      {/if}
      <div class="scope-text">
        <h2 dir="auto">{store.scopeTitle}</h2>
        <span class="sub">
          {#if store.scopeUnread > 0}{store.scopeUnread} unread{:else if store.scope.kind === 'starred'}{store.tree.starred} starred{:else}All caught up{/if}
        </span>
      </div>
    </div>
    <div class="actions">
      <button class="icon-btn" title="Mark all as read (Shift+A)" onclick={() => store.markAllRead()} disabled={store.scopeUnread === 0}>
        <Icon name="check-check" size={17} />
      </button>
      <button class="icon-btn" title="Refresh (R)" onclick={() => store.refreshScope()}>
        <Icon name="refresh" size={16} />
      </button>
      <button class="icon-btn" title="View options" onclick={viewMenu}>
        <Icon name={store.settings.viewMode} size={17} />
      </button>
    </div>
  </div>
  <div class="tools">
    <label class="search">
      <Icon name="search" size={14} />
      <input
        bind:this={searchInput}
        bind:value={draft}
        {oninput}
        onkeydown={(e) => {
          if (e.key === 'Enter') {
            clearTimeout(timer);
            store.setQuery(draft);
          }
        }}
        type="search"
        placeholder="Search titles and text"
        spellcheck="false"
        autocomplete="off"
      />
      {#if draft}
        <button class="clear" aria-label="Clear search" onclick={clear}><Icon name="x" size={12} stroke={2.4} /></button>
      {/if}
    </label>
    <button class="chip" class:active={store.filter !== 'all'} onclick={filterMenu}>
      <Icon name="filter" size={12} />
      {filterLabel}
      <Icon name="chevron-down" size={12} stroke={2.2} />
    </button>
    <button class="chip" class:active={store.dateFiltered} onclick={dateMenu} title="Publication date">
      <Icon name="calendar" size={12} />
      <span class="chip-text">{dateLabel}</span>
      <Icon name="chevron-down" size={12} stroke={2.2} />
    </button>
  </div>
</header>

<style>
  .list-head {
    padding: 12px 12px 10px 16px;
    border-bottom: 1px solid var(--border);
    display: grid;
    gap: 10px;
  }
  .title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }
  .scope {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .scope-icon {
    display: grid;
    place-items: center;
    width: 18px;
    color: var(--accent);
  }
  .scope-text {
    min-width: 0;
    display: grid;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.015em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 11.5px;
    color: var(--text-3);
  }
  .actions {
    display: flex;
    gap: 2px;
    flex: none;
  }
  .tools {
    display: flex;
    gap: 6px;
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 8px 0 10px;
    border-radius: 8px;
    background: var(--bg-muted);
    border: 1px solid var(--border);
    color: var(--text-3);
    transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
    min-width: 0;
  }
  .search:focus-within {
    background: var(--bg-input);
    border-color: rgb(var(--accent-rgb) / 0.6);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search input {
    flex: 1;
    /* Without width: 0 the input's natural width (about 150px) counts toward the
       column's minimum, and the chips then push the list wider than its pane. */
    width: 0;
    min-width: 0;
    border: none;
    background: transparent;
    font-size: 12.5px;
    color: var(--text);
    outline: none;
  }
  .search input::placeholder {
    color: var(--text-3);
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  .clear {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border: none;
    border-radius: 50%;
    background: var(--border-strong);
    color: var(--bg-content);
    padding: 0;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--bg-muted);
    color: var(--text-2);
    font-size: 12px;
    font-weight: 500;
    flex: none;
  }
  .chip:hover {
    color: var(--text);
    background: var(--bg-hover);
  }
  .chip.active {
    color: var(--accent);
    background: var(--accent-softer);
    border-color: rgb(var(--accent-rgb) / 0.25);
  }
  .chip-text {
    max-width: 112px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
