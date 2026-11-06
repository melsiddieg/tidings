<script lang="ts">
  import { iconUrl } from '../lib/api';
  import type { Feed } from '../lib/types';
  import { avatarHue, initial } from '../lib/util';

  let { feed, size = 16 }: { feed: Feed | undefined; size?: number } = $props();

  let failed = $state(false);
  const src = $derived(feed ? iconUrl(feed.id, feed.iconVersion) : null);
  const name = $derived(feed?.title ?? '?');

  $effect(() => {
    src;
    failed = false;
  });
</script>

{#if src && !failed}
  <img class="feed-icon" {src} alt="" width={size} height={size} onerror={() => (failed = true)} />
{:else}
  <span
    class="feed-icon letter"
    style:--hue={avatarHue(name)}
    style:width="{size}px"
    style:height="{size}px"
    style:font-size="{Math.round(size * 0.62)}px">{initial(name)}</span
  >
{/if}

<style>
  .feed-icon {
    flex: none;
    display: block;
    border-radius: 4px;
    object-fit: contain;
  }
  .letter {
    display: grid;
    place-items: center;
    font-weight: 700;
    line-height: 1;
    color: hsl(var(--hue) 45% 38%);
    background: hsl(var(--hue) 60% 90%);
  }
  :global([data-theme='dark']) .letter {
    color: hsl(var(--hue) 70% 78%);
    background: hsl(var(--hue) 30% 24%);
  }
</style>
