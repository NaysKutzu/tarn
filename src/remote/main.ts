import './remote.css';
import { mount } from 'svelte';
import RemoteApp from './RemoteApp.svelte';

mount(RemoteApp, { target: document.getElementById('app')! });
