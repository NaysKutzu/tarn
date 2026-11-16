import { api } from './api';

// Phones only: record the viewport / safe-area numbers so a layout problem
// (e.g. a gap under the tab bar) can be read from "Copy diagnostics".
export function probeLayout() {
  if (!window.matchMedia('(max-width: 700px)').matches) return;
  const run = () => {
    try {
      const p = document.createElement('div');
      p.style.cssText =
        'position:fixed;left:0;top:0;visibility:hidden;pointer-events:none;' +
        'padding:env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left)';
      document.body.appendChild(p);
      const cs = getComputedStyle(p);
      const env = `${cs.paddingTop}/${cs.paddingRight}/${cs.paddingBottom}/${cs.paddingLeft}`;
      p.remove();
      const r = (sel: string) => {
        const e = document.querySelector(sel);
        if (!e) return 'none';
        const b = e.getBoundingClientRect();
        return `${Math.round(b.top)}-${Math.round(b.bottom)}`;
      };
      const vv = window.visualViewport;
      void api.diagNote(
        `layout inner=${innerWidth}x${innerHeight} vv=${vv ? Math.round(vv.height) : '-'} ` +
          `screen=${screen.width}x${screen.height} client=${document.documentElement.clientHeight} ` +
          `dpr=${devicePixelRatio} env(t/r/b/l)=${env} app=${r('.app')} sidebar=${r('.sidebar')} ` +
          `player=${r('.player')} body=${Math.round(document.body.getBoundingClientRect().height)}`,
      );
    } catch {
      /* diagnostics only */
    }
  };
  setTimeout(run, 2500);
  window.addEventListener('orientationchange', () => setTimeout(run, 600));
}
