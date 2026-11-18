// Easter egg: typing "iamasecurityengineer" anywhere spins the whole app,
// then flips it into EXTREME MODE (rainbow accent, wobbling UI, bouncing
// cover art). Typing it again turns it off.
import { toast } from './toast';

const CODE = 'iamasecurityengineer';
let buf = '';
let busy = false;

function setExtreme(on: boolean) {
  document.documentElement.classList.toggle('extreme', on);
}

function trigger() {
  if (busy) return;
  busy = true;
  const root = document.documentElement;
  const turningOn = !root.classList.contains('extreme');
  root.classList.add('egg-spin');
  window.setTimeout(() => {
    root.classList.remove('egg-spin');
    setExtreme(turningOn);
    toast(turningOn ? 'EXTREME MODE ON 🔥 (type it again to leave)' : 'Back to normal', 'info');
    busy = false;
  }, 2600);
}

export function initEasterEgg() {
  window.addEventListener(
    'keydown',
    (e) => {
      if (e.ctrlKey || e.metaKey || e.altKey || e.key.length !== 1) return;
      buf = (buf + e.key.toLowerCase()).slice(-CODE.length);
      if (buf === CODE) {
        buf = '';
        trigger();
      }
    },
    true,
  );
}
