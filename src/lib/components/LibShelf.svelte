<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t as tr } from '../i18n.svelte';
  import Hi from './Hi.svelte';
  import ScrollRow from './ScrollRow.svelte';
  import { ArrowLeft01Icon, ArrowRight01Icon } from '../icons';

  let {
    title,
    children,
    trailing,
  }: {
    title: string;
    children: Snippet;
    /** Optional controls next to the shelf title (e.g. download). */
    trailing?: Snippet;
  } = $props();

  let scroller = $state<{ page: (dir: -1 | 1) => void } | null>(null);
  let canLeft = $state(false);
  let canRight = $state(false);
</script>

<section class="shelf">
  <div class="shelf-head">
    <h2 class="shelf-title">{title}</h2>
    <div class="lib-shelf-end">
      {#if trailing}{@render trailing()}{/if}
      {#if canLeft || canRight}
        <div class="shelf-nav">
          <button
            class="shelf-nav-btn"
            type="button"
            title={tr('Previous')}
            aria-label={tr('Previous')}
            disabled={!canLeft}
            onclick={() => scroller?.page(-1)}
          >
            <Hi icon={ArrowLeft01Icon} size={18} />
          </button>
          <button
            class="shelf-nav-btn"
            type="button"
            title={tr('Next')}
            aria-label={tr('Next')}
            disabled={!canRight}
            onclick={() => scroller?.page(1)}
          >
            <Hi icon={ArrowRight01Icon} size={18} />
          </button>
        </div>
      {/if}
    </div>
  </div>
  <ScrollRow bind:this={scroller} bind:canLeft bind:canRight>
    {@render children()}
  </ScrollRow>
</section>

<style>
  .lib-shelf-end {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 0 0 auto;
  }
</style>
