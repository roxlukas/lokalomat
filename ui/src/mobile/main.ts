import { mount } from 'svelte';
import '@fontsource-variable/inter/wght.css';
import '@fontsource-variable/jetbrains-mono/wght.css';
import '../styles/fonts.css';
import '../styles/tokens.css';
import '../styles/base.css';
import '../styles/mobile.css';
import MobileApp from './MobileApp.svelte';
import { setLocale } from '../lib/i18n/index.svelte';

// The phone follows its system language (Polish or English).
setLocale(navigator.language.toLowerCase().startsWith('pl') ? 'pl' : 'en');

const target = document.getElementById('app');
if (!target) throw new Error('#app element missing');

export default mount(MobileApp, { target });
