import { createApp } from 'vue';
import { createI18n } from 'vue-i18n';
import App from './App.vue';

import en from './locales/en';
import zh from './locales/zh';

const savedLocale = localStorage.getItem('docs-locale') || 'en';

const i18n = createI18n({
  legacy: false,
  locale: savedLocale,
  fallbackLocale: 'en',
  messages: { en, zh }
});

createApp(App).use(i18n).mount('#app');
