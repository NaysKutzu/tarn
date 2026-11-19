import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import Mini from './Mini.svelte';
import { probeLayout } from './lib/layoutProbe';
import { initEasterEgg } from './lib/easter';
import { initTheme } from './lib/theme';

initTheme();

// The mini player is a second window that loads this same page with ?mini=1.
const mini = new URLSearchParams(location.search).has('mini');

mount(mini ? Mini : App, { target: document.getElementById('app')! });

if (!mini) probeLayout();
initEasterEgg();
