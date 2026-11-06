<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { store } from '../lib/store.svelte';
  import type { Tab } from '../lib/types';
  import { countLabel } from '../lib/util';
  import FeedIcon from './FeedIcon.svelte';

  function tabMenu(e: MouseEvent, tab: Tab) {
    const items = [];
    if (tab.kind !== 'main') {
      items.push({ label: 'Close tab', icon: 'x', action: () => store.closeTab(tab.id) });
    }
    items.push({ label: 'Close other tabs', action: () => store.closeOtherTabs(tab.id), disabled: store.tabs.length < 3 && tab.kind !== 'main' });
    if (tab.kind === 'web') {
      items.push({ separator: true });
      items.push({ label: 'Open in browser', icon: 'globe', action: () => store.openExternal(tab.url) });
    }
    store.openMenu(e, items);
  }

  function onwheel(e: WheelEvent) {
    const el = e.currentTarget as HTMLElement;
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) el.scrollLeft += e.deltaY;
  }
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="tabs" role="tablist" data-tauri-drag-region {onwheel}>
    {#each store.tabs as tab (tab.id)}
      {@const active = tab.id === store.activeTabId}
      <div
        class="tab"
        class:active
        class:main={tab.kind === 'main'}
        role="tab"
        tabindex="-1"
        aria-selected={active}
        title={tab.kind === 'main' ? 'Feeds' : tab.title}
        onmousedown={(e) => e.button === 0 && store.activateTab(tab.id)}
        onauxclick={(e) => {
          if (e.button === 1) {
            e.preventDefault();
            store.closeTab(tab.id);
          }
        }}
        oncontextmenu={(e) => tabMenu(e, tab)}
      >
        {#if tab.kind === 'main'}
          <span class="tab-icon accent"><Icon name="rss" size={14} stroke={2.2} /></span>
          <span class="tab-title">Feeds</span>
          {#if store.unreadTotal > 0}<span class="tab-count">{countLabel(store.unreadTotal)}</span>{/if}
        {:else if tab.kind === 'article'}
          <span class="tab-icon"><FeedIcon feed={tab.feedId != null ? store.feedById.get(tab.feedId) : undefined} size={14} /></span>
          <span class="tab-title">{tab.title}</span>
        {:else}
          <span class="tab-icon"><Icon name="book" size={14} /></span>
          <span class="tab-title">{tab.title}</span>
        {/if}
        {#if tab.kind !== 'main'}
          <button
            class="tab-close"
            aria-label="Close tab"
            onmousedown={(e) => e.stopPropagation()}
            onclick={(e) => {
              e.stopPropagation();
              store.closeTab(tab.id);
            }}><Icon name="x" size={12} stroke={2.2} /></button
          >
        {/if}
      </div>
    {/each}
  </div>

  <div class="drag" data-tauri-drag-region></div>

  {#if store.progress}
    <div class="progress" title="Refreshing feeds">
      <span class="spinner"></span>
      <span>{store.progress.done} / {store.progress.total}</span>
      <span class="bar"><span style:width="{(store.progress.done / Math.max(1, store.progress.total)) * 100}%"></span></span>
    </div>
  {/if}
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--titlebar-h);
    padding: 0 10px 0 10px;
    background: var(--bg-titlebar);
    min-width: 0;
  }
  :global([data-platform='macos']) .titlebar {
    padding-left: 84px;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
    height: 100%;
    padding: 0 2px;
  }
  .tabs::-webkit-scrollbar {
    display: none;
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    min-width: 96px;
    max-width: 220px;
    flex: 0 1 200px;
    padding: 0 8px 0 10px;
    border-radius: 9px;
    color: var(--text-2);
    font-size: 12.5px;
    font-weight: 500;
    transition: background 0.15s var(--ease), color 0.15s var(--ease), box-shadow 0.15s var(--ease);
  }
  .tab.main {
    flex: none;
    min-width: 0;
    padding-right: 10px;
  }
  .tab:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .tab.active {
    background: var(--bg-elevated);
    color: var(--text);
    box-shadow: var(--shadow-sm);
  }
  .tab:not(.active):not(:hover) + .tab:not(.active):not(:hover)::before {
    content: '';
    position: absolute;
    left: -3px;
    top: 9px;
    bottom: 9px;
    width: 1px;
    background: var(--border-strong);
  }
  .tab-icon {
    display: grid;
    place-items: center;
    flex: none;
    color: var(--text-3);
  }
  .tab-icon.accent {
    color: var(--accent);
  }
  .tab-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .tab.main .tab-title {
    flex: none;
  }
  .tab-count {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }
  .tab-close {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: var(--text-3);
    opacity: 0;
    flex: none;
    transition: opacity 0.12s, background 0.12s;
  }
  .tab:hover .tab-close,
  .tab.active .tab-close {
    opacity: 1;
  }
  .tab-close:hover {
    background: var(--bg-press);
    color: var(--text);
  }
  .drag {
    flex: 1;
    align-self: stretch;
    min-width: 40px;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11.5px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
    flex: none;
    animation: fade-up 0.25s var(--ease);
  }
  .bar {
    width: 64px;
    height: 4px;
    border-radius: 2px;
    background: var(--border);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 2px;
    transition: width 0.3s var(--ease);
  }
</style>
