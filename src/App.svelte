<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { store } from './lib/store.svelte';
  import { isTyping } from './lib/util';
  import TitleBar from './components/TitleBar.svelte';
  import Workspace from './components/Workspace.svelte';
  import ArticleView from './components/ArticleView.svelte';
  import ReaderTab from './components/ReaderTab.svelte';
  import ContextMenu from './components/ContextMenu.svelte';
  import Toasts from './components/Toasts.svelte';
  import PlayerBar from './components/PlayerBar.svelte';
  import { player } from './lib/player.svelte';
  import AddFeedDialog from './components/AddFeedDialog.svelte';
  import FeedDialog from './components/FeedDialog.svelte';
  import FolderDialog from './components/FolderDialog.svelte';
  import ConfirmDialog from './components/ConfirmDialog.svelte';
  import SettingsDialog from './components/SettingsDialog.svelte';
  import ShortcutsDialog from './components/ShortcutsDialog.svelte';
  import DateRangeDialog from './components/DateRangeDialog.svelte';

  onMount(() => {
    store.init();
  });

  // Unread count on the dock / launcher icon.
  $effect(() => {
    if (!store.ready) return;
    const count = store.settings.dockBadge ? store.unreadTotal : 0;
    api.setBadge(count).catch(() => {});
  });

  const isMac = $derived(store.info?.platform === 'macos');

  function scrollActive(direction: 1 | -1): boolean {
    const el = document.querySelector<HTMLElement>('.tab-panel.active .article-scroll');
    if (!el) return false;
    const atEnd = el.scrollTop + el.clientHeight >= el.scrollHeight - 4;
    if (direction > 0 && atEnd) return false;
    el.scrollBy({ top: direction * el.clientHeight * 0.85, behavior: 'smooth' });
    return true;
  }

  function onkeydown(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key;

    if (key === 'Escape') {
      if (store.menu) store.menu = null;
      else if (store.dialog) store.dialog = null;
      else if (isTyping(e.target)) (e.target as HTMLElement).blur();
      return;
    }
    if (store.dialog || store.menu) return;

    // Tab switching works everywhere.
    if (e.ctrlKey && key === 'Tab') {
      e.preventDefault();
      store.cycleTab(e.shiftKey ? -1 : 1);
      return;
    }
    if (mod && e.shiftKey && (key === ']' || key === '}' || key === '[' || key === '{')) {
      e.preventDefault();
      store.cycleTab(key === ']' || key === '}' ? 1 : -1);
      return;
    }
    if (mod && !e.shiftKey && !e.altKey && /^[1-9]$/.test(key)) {
      e.preventDefault();
      const tab = key === '9' ? store.tabs[store.tabs.length - 1] : store.tabs[Number(key) - 1];
      if (tab) store.activateTab(tab.id);
      return;
    }

    // On macOS these come from the native menu instead.
    if (mod && !isMac) {
      const k = key.toLowerCase();
      const map: Record<string, string> = {
        w: 'close-tab',
        n: e.shiftKey ? 'new-folder' : 'add-feed',
        ',': 'settings',
        f: 'find',
        r: 'refresh-all',
        '/': 'shortcuts',
      };
      if (map[k]) {
        e.preventDefault();
        store.onMenu(map[k]);
      }
      return;
    }
    if (mod || e.altKey || isTyping(e.target)) return;

    const onMain = store.activeTabId === 'main';
    switch (key) {
      case 'j':
      case 'ArrowDown':
        if (!onMain) return;
        store.moveSelection(1);
        break;
      case 'k':
      case 'ArrowUp':
        if (!onMain) return;
        store.moveSelection(-1);
        break;
      case 'n':
      case '+':
      case '=':
        if (!onMain) store.activateTab('main');
        store.nextUnread(1);
        break;
      case 'p':
      case '-':
        if (!onMain) store.activateTab('main');
        store.nextUnread(-1);
        break;
      case ' ':
        if (!scrollActive(e.shiftKey ? -1 : 1) && onMain && !e.shiftKey) store.nextUnread(1);
        break;
      case 'Enter':
      case 'o':
        if (onMain && store.selectedId != null) store.openArticleTab(store.selectedId);
        else return;
        break;
      case 'v':
      case 'b':
        store.send('browser');
        break;
      case 'w':
        store.send('watch');
        break;
      case 's':
        store.send('star');
        break;
      case 'm':
      case 'u':
        store.send('read');
        break;
      case 'f':
        store.send('full-text');
        break;
      case 'i':
        store.send('images');
        break;
      case 'c':
        store.send('copy-link');
        break;
      case 'Delete':
        store.send('delete');
        break;
      case 'r':
        store.refreshScope();
        break;
      case 'R':
        store.refresh();
        break;
      case 'A':
        store.markAllRead();
        break;
      case 'P':
        if (!player.episode) return;
        player.toggle();
        break;
      case 'x':
        if (!onMain) store.closeTab(store.activeTabId);
        else return;
        break;
      case '/':
        store.onMenu('find');
        break;
      case '?':
        store.dialog = { kind: 'shortcuts' };
        break;
      case 'ArrowLeft':
      case 'ArrowRight':
        return;
      default:
        return;
    }
    e.preventDefault();
  }

  // Like native toolbars, buttons outside dialogs do not take focus on click.
  function onmousedown(e: MouseEvent) {
    const button = (e.target as Element | null)?.closest?.('button');
    if (button && !button.closest('.modal, .menu')) e.preventDefault();
  }

  // Hide the web view's default context menu, except where it is useful (text fields, selected text).
  function oncontextmenu(e: MouseEvent) {
    if (isTyping(e.target)) return;
    const selection = window.getSelection();
    if (selection && !selection.isCollapsed) return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} {oncontextmenu} {onmousedown} />

<div class="app" class:ready={store.ready}>
  <TitleBar />
  <main class="panels">
    {#each store.tabs as tab (tab.id)}
      <section class="tab-panel" class:active={tab.id === store.activeTabId}>
        {#if tab.kind === 'main'}
          <Workspace />
        {:else if tab.kind === 'article'}
          <ArticleView articleId={tab.articleId} mode="tab" tabId={tab.id} active={tab.id === store.activeTabId} />
        {:else}
          <ReaderTab {tab} active={tab.id === store.activeTabId} />
        {/if}
      </section>
    {/each}
  </main>
  <PlayerBar />

  {#if store.dialog?.kind === 'add-feed'}
    <AddFeedDialog dialog={store.dialog} />
  {:else if store.dialog?.kind === 'feed'}
    <FeedDialog feedId={store.dialog.feedId} />
  {:else if store.dialog?.kind === 'folder'}
    <FolderDialog dialog={store.dialog} />
  {:else if store.dialog?.kind === 'confirm'}
    <ConfirmDialog dialog={store.dialog} />
  {:else if store.dialog?.kind === 'settings'}
    <SettingsDialog section={store.dialog.section} />
  {:else if store.dialog?.kind === 'shortcuts'}
    <ShortcutsDialog />
  {:else if store.dialog?.kind === 'date-range'}
    <DateRangeDialog />
  {/if}

  {#if store.menu}
    <ContextMenu menu={store.menu} />
  {/if}
  <Toasts />
</div>

<style>
  .app {
    display: grid;
    grid-template-rows: var(--titlebar-h) minmax(0, 1fr) auto;
    height: 100vh;
    background: var(--bg-titlebar);
    opacity: 0;
    transition: opacity 0.25s var(--ease);
  }
  .app.ready {
    opacity: 1;
  }
  .panels {
    position: relative;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-content);
    border-top: 1px solid var(--border);
  }
  .tab-panel {
    position: absolute;
    inset: 0;
    display: none;
  }
  .tab-panel.active {
    display: block;
  }
</style>
