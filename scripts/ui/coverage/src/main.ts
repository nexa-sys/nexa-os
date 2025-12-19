import { createApp } from 'vue';
import { createI18n } from 'vue-i18n';
import App from './App.vue';
import { en } from './locales/en';
import { zh } from './locales/zh';

// Create i18n instance
const i18n = createI18n({
  legacy: false,
  locale: localStorage.getItem('coverage-locale') || navigator.language.startsWith('zh') ? 'zh' : 'en',
  fallbackLocale: 'en',
  messages: { en, zh }
});

// Create and mount app
const app = createApp(App);
app.use(i18n);
app.mount('#app');
