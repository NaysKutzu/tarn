<script lang="ts">
  import { t as tr } from '../i18n.svelte';
</script>

<div class="home-build" role="status" aria-live="polite" aria-busy="true">
  <div class="home-build-stage" aria-hidden="true">
    <div class="home-build-orb a"></div>
    <div class="home-build-orb b"></div>
    <div class="home-build-mark">
      <img src="/logo.png" width="72" height="72" alt="" />
      <span class="home-build-ring"></span>
    </div>
    <div class="home-build-shelves">
      {#each { length: 3 } as _, i (i)}
        <div class="home-build-shelf" style="--i:{i}">
          {#each { length: 5 } as _, j (j)}
            <span class="home-build-tile" style="--j:{j}"></span>
          {/each}
        </div>
      {/each}
    </div>
  </div>
  <h2 class="home-build-title">{tr('Please wait')}</h2>
  <p class="home-build-copy">
    {tr('Tarn is building your awesome experience. This might take a bit of time, so be patient.')}
  </p>
</div>

<style>
  .home-build {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    min-height: min(62vh, 560px);
    padding: 48px 24px 64px;
    gap: 12px;
  }
  .home-build-stage {
    position: relative;
    width: min(420px, 100%);
    height: 220px;
    margin-bottom: 12px;
    display: grid;
    place-items: center;
  }
  .home-build-orb {
    position: absolute;
    border-radius: 50%;
    filter: blur(28px);
    opacity: 0.45;
    pointer-events: none;
  }
  .home-build-orb.a {
    width: 160px;
    height: 160px;
    background: rgb(var(--accent-rgb, 232 180 120) / 0.35);
    animation: home-build-drift 5.5s ease-in-out infinite;
  }
  .home-build-orb.b {
    width: 120px;
    height: 120px;
    background: rgb(255 255 255 / 0.08);
    translate: 48px 28px;
    animation: home-build-drift 7s ease-in-out infinite reverse;
  }
  .home-build-mark {
    position: relative;
    z-index: 2;
    width: 88px;
    height: 88px;
    display: grid;
    place-items: center;
    animation: home-build-breathe 2.4s var(--ease, ease) infinite;
  }
  .home-build-mark img {
    width: 72px;
    height: 72px;
    object-fit: contain;
    position: relative;
    z-index: 1;
  }
  .home-build-ring {
    position: absolute;
    inset: -6px;
    border-radius: 50%;
    border: 1.5px solid rgb(var(--accent-rgb, 232 180 120) / 0.45);
    animation: home-build-ring 2.4s var(--ease, ease) infinite;
  }
  .home-build-shelves {
    position: absolute;
    inset: auto 0 8px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    z-index: 1;
    opacity: 0.85;
  }
  .home-build-shelf {
    display: flex;
    gap: 8px;
    justify-content: center;
    animation: home-build-rise 1.8s var(--ease, ease) both;
    animation-delay: calc(var(--i) * 160ms + 120ms);
  }
  .home-build-tile {
    width: 52px;
    height: 52px;
    border-radius: 10px;
    background: linear-gradient(
      145deg,
      rgb(255 255 255 / 0.1),
      rgb(255 255 255 / 0.03)
    );
    border: 1px solid rgb(255 255 255 / 0.06);
    animation: home-build-tile 2.2s var(--ease, ease) infinite;
    animation-delay: calc(var(--i) * 120ms + var(--j) * 70ms);
  }
  .home-build-title {
    margin: 0;
    font-size: 28px;
    line-height: 1.15;
    font-weight: 700;
    letter-spacing: -0.03em;
    animation: home-build-fade 0.7s var(--ease, ease) both;
  }
  .home-build-copy {
    margin: 0;
    max-width: 34rem;
    font-size: 15px;
    line-height: 1.45;
    color: var(--text-2, #a8a29a);
    animation: home-build-fade 0.7s var(--ease, ease) 0.12s both;
  }

  @keyframes home-build-breathe {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.04); }
  }
  @keyframes home-build-ring {
    0%, 100% { transform: scale(1); opacity: 0.7; }
    50% { transform: scale(1.12); opacity: 0.25; }
  }
  @keyframes home-build-drift {
    0%, 100% { transform: translate(0, 0); }
    50% { transform: translate(12px, -10px); }
  }
  @keyframes home-build-rise {
    from { opacity: 0; transform: translateY(14px); }
    to { opacity: 1; transform: none; }
  }
  @keyframes home-build-tile {
    0%, 100% { opacity: 0.35; transform: scale(0.96); }
    40% { opacity: 0.95; transform: scale(1); }
    70% { opacity: 0.55; }
  }
  @keyframes home-build-fade {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: none; }
  }

  @media (prefers-reduced-motion: reduce) {
    .home-build-mark,
    .home-build-ring,
    .home-build-orb,
    .home-build-shelf,
    .home-build-tile,
    .home-build-title,
    .home-build-copy {
      animation: none;
    }
    .home-build-tile { opacity: 0.55; }
  }
</style>
