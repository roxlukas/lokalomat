import { mount } from 'svelte';
import '@fontsource-variable/inter/wght.css';
import '@fontsource-variable/jetbrains-mono/wght.css';
import './styles/fonts.css';
import './styles/tokens.css';
import './styles/base.css';
// The edition registers its views before the app starts (ADR 0035).
import '@edition';
import App from './App.svelte';
import { installErrorForwarding } from './lib/errors';

installErrorForwarding();

const target = document.getElementById('app');
if (!target) throw new Error('#app element missing');

export default mount(App, { target });
