<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { copyText, isVideoLink } from '../lib/util';

  // `html` is already sanitized by the Rust backend; the CSP blocks scripts as a second layer.
  let { html, dir = 'auto' }: { html: string; dir?: 'ltr' | 'rtl' | 'auto' } = $props();

  let el: HTMLDivElement | undefined = $state();

  function linkFrom(e: MouseEvent): string | null {
    const target = e.target as Element | null;
    const a = target?.closest?.('a[href]') as HTMLAnchorElement | null;
    return a?.href || null;
  }

  function onclick(e: MouseEvent) {
    const href = linkFrom(e);
    if (!href) return;
    e.preventDefault();
    if (e.shiftKey) store.openLink(href, 'browser');
    else if (e.metaKey || e.ctrlKey) store.openLink(href, 'background');
    else store.openLink(href);
  }

  function onauxclick(e: MouseEvent) {
    if (e.button !== 1) return;
    const href = linkFrom(e);
    if (!href) return;
    e.preventDefault();
    store.openLink(href, 'background');
  }

  function oncontextmenu(e: MouseEvent) {
    const href = linkFrom(e);
    if (!href) return;
    const watch = isVideoLink(href)
      ? [{ label: `Watch in ${store.videoPlayer?.name ?? 'video player'}`, icon: 'video', action: () => store.watchVideo(href) }]
      : [];
    store.openMenu(e, [
      ...watch,
      { label: 'Open in reader tab', icon: 'book', action: () => store.openWebTab(href) },
      { label: 'Open in background tab', icon: 'plus', action: () => store.openWebTab(href, true) },
      { label: 'Open in browser', icon: 'globe', action: () => store.openLink(href, 'browser') },
      { separator: true },
      { label: 'Copy link', icon: 'link', action: () => copyText(href).then(() => store.toast('Link copied')) },
    ]);
  }

  // Hide images that fail to load instead of showing a broken icon.
  $effect(() => {
    const node = el;
    if (!node) return;
    const onError = (ev: Event) => {
      const t = ev.target as HTMLElement;
      if (t.tagName === 'IMG') t.classList.add('broken');
    };
    node.addEventListener('error', onError, true);
    return () => node.removeEventListener('error', onError, true);
  });
</script>

<div class="prose" {dir} bind:this={el} {onclick} {onauxclick} {oncontextmenu} role="presentation">
  {@html html}
</div>

<style>
  .prose :global(img.broken) {
    display: none;
  }
</style>
