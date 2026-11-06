<script lang="ts">
  import { ICONS } from './icons';

  let {
    name,
    size = 16,
    stroke = 1.75,
    fill = false,
    class: className = '',
  }: { name: string; size?: number; stroke?: number; fill?: boolean; class?: string } = $props();
</script>

<svg
  class="icon {className}"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={fill ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true">{@html ICONS[name] ?? ''}</svg
>

<style>
  .icon {
    flex: none;
    display: block;
  }
</style>
