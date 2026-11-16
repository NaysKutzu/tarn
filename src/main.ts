import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { probeLayout } from './lib/layoutProbe';

mount(App, { target: document.getElementById('app')! });

probeLayout();
