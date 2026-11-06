<script lang="ts">
  import Modal from './Modal.svelte';
  import { store } from '../lib/store.svelte';

  const mod = $derived(store.info?.platform === 'macos' ? '⌘' : 'Ctrl');

  const groups = $derived([
    {
      title: 'Reading',
      keys: [
        [['J', '↓'], 'Next article'],
        [['K', '↑'], 'Previous article'],
        [['N', '+'], 'Next unread article'],
        [['P', '−'], 'Previous unread article'],
        [['Space'], 'Scroll, then next unread'],
        [['O', 'Enter'], 'Open article in a tab'],
        [['V'], 'Open in browser'],
        [['F'], 'Load full article'],
        [['I'], 'Show or hide images'],
      ],
    },
    {
      title: 'Articles',
      keys: [
        [['S'], 'Star or unstar'],
        [['M'], 'Mark read or unread'],
        [['C'], 'Copy link'],
        [['⇧', 'P'], 'Play or pause podcast'],
        [['⇧', 'A'], 'Mark all as read'],
        [['Delete'], 'Delete article'],
        [['R'], 'Refresh this feed or folder'],
        [['⇧', 'R'], 'Refresh all feeds'],
      ],
    },
    {
      title: 'Tabs and app',
      keys: [
        [[mod, 'W'], 'Close tab'],
        [['Ctrl', 'Tab'], 'Next tab'],
        [[mod, '1–9'], 'Go to tab'],
        [['X'], 'Close reader tab'],
        [['/'], 'Search'],
        [[mod, 'N'], 'Add feed'],
        [[mod, ','], 'Settings'],
        [['?'], 'This list'],
      ],
    },
  ] as { title: string; keys: [string[], string][] }[]);
</script>

<Modal title="Keyboard shortcuts" width={720}>
  <div class="cols">
    {#each groups as g (g.title)}
      <section>
        <h3>{g.title}</h3>
        {#each g.keys as [keys, label] (label)}
          <div class="row">
            <span class="label">{label}</span>
            <span class="keys">
              {#each keys as k, i (i)}<kbd>{k}</kbd>{/each}
            </span>
          </div>
        {/each}
      </section>
    {/each}
  </div>
</Modal>

<style>
  .cols {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 24px;
  }
  h3 {
    margin: 0 0 10px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 5px 0;
    font-size: 12.5px;
  }
  .label {
    color: var(--text-2);
  }
  .keys {
    display: flex;
    gap: 3px;
    flex: none;
  }
</style>
