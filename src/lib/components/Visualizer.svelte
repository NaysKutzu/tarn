<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  let {
    mode = 'bars',
    playing = false,
  }: {
    /** "bars" = spectrum bars, "wave" = smooth spectrum curve. */
    mode?: 'bars' | 'wave';
    playing?: boolean;
  } = $props();

  const BANDS = 64;
  let canvas: HTMLCanvasElement | undefined = $state();
  let levels = new Float32Array(BANDS);
  let raf = 0;
  let busy = false;
  let last = 0;
  let alive = true;

  function color(alpha: number): string {
    const rgb = getComputedStyle(document.documentElement).getPropertyValue('--accent-rgb').trim();
    return rgb ? `rgba(${rgb.replace(/\s+/g, ',').replace(/,+/g, ',')},${alpha})` : `rgba(255,255,255,${alpha})`;
  }

  function draw() {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const grad = ctx.createLinearGradient(0, h, 0, 0);
    grad.addColorStop(0, color(0.9));
    grad.addColorStop(1, 'rgba(255,255,255,0.32)');
    ctx.fillStyle = grad;
    if (mode === 'wave') {
      const pts = Array.from(levels);
      const step = w / (pts.length - 1);
      ctx.beginPath();
      ctx.moveTo(0, h);
      for (let i = 0; i < pts.length; i++) {
        const x = i * step, y = h - pts[i] * h;
        if (i === 0) ctx.lineTo(x, y);
        else {
          const px = (i - 1) * step, py = h - pts[i - 1] * h;
          ctx.quadraticCurveTo(px, py, (px + x) / 2, (py + y) / 2);
        }
      }
      ctx.lineTo(w, h);
      ctx.closePath();
      ctx.fill();
    } else {
      const gap = Math.max(2, w / BANDS * 0.28);
      const bw = (w - gap * (BANDS - 1)) / BANDS;
      for (let i = 0; i < BANDS; i++) {
        const bh = Math.max(2, levels[i] * h);
        const x = i * (bw + gap);
        const r = Math.min(bw / 2, 3);
        ctx.beginPath();
        ctx.roundRect(x, h - bh, bw, bh, [r, r, 0, 0]);
        ctx.fill();
      }
    }
  }

  function frame(t: number) {
    if (!alive) return;
    raf = requestAnimationFrame(frame);
    if (t - last < 28) return; // ~35 fps is plenty
    last = t;
    if (!playing || document.hidden) {
      let any = false;
      for (let i = 0; i < BANDS; i++) {
        levels[i] *= 0.88;
        if (levels[i] > 0.002) any = true;
      }
      if (any) draw();
      return;
    }
    if (busy) return;
    busy = true;
    invoke<number[]>('viz_frame', { bands: BANDS })
      .then((b) => {
        if (b && b.length === BANDS) {
          for (let i = 0; i < BANDS; i++) {
            // Fast attack, slow release.
            const v = b[i];
            levels[i] = v > levels[i] ? levels[i] * 0.35 + v * 0.65 : levels[i] * 0.86 + v * 0.14;
          }
        }
        draw();
      })
      .catch(() => {})
      .finally(() => { busy = false; });
  }

  onMount(() => {
    void invoke('set_visualizer', { on: true }).catch(() => {});
    raf = requestAnimationFrame(frame);
  });
  onDestroy(() => {
    alive = false;
    cancelAnimationFrame(raf);
    void invoke('set_visualizer', { on: false }).catch(() => {});
  });
</script>

<canvas class="viz viz-{mode}" bind:this={canvas} aria-hidden="true"></canvas>
