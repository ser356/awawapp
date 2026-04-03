import { createI18n } from 'vue-i18n';
import en from './en';
import es from './es';
import fr from './fr';
import de from './de';
import it from './it';
import pt from './pt';
import ru from './ru';
import ja from './ja';
import ko from './ko';
import zh from './zh';
import nl from './nl';
import sv from './sv';
import pl from './pl';
import tr from './tr';
import ar from './ar';

const supportedLocales = ['en', 'es', 'fr', 'de', 'it', 'pt', 'ru', 'ja', 'ko', 'zh', 'nl', 'sv', 'pl', 'tr', 'ar'] as const;

type SupportedLocale = typeof supportedLocales[number];

function isSupportedLocale(lang: string): lang is SupportedLocale {
  return (supportedLocales as readonly string[]).includes(lang);
}

// Get saved language or detect from browser
function getDefaultLocale(): SupportedLocale {
  // Check localStorage first
  const saved = localStorage.getItem('awawapp-language');
  if (saved && isSupportedLocale(saved)) {
    return saved;
  }

  // Detect from browser
  const browserLang = navigator.language.split('-')[0];
  if (isSupportedLocale(browserLang)) {
    return browserLang;
  }

  return 'en';
}

const i18n = createI18n({
  legacy: false,
  locale: getDefaultLocale(),
  fallbackLocale: 'en',
  messages: {
    en,
    es,
    fr,
    de,
    it,
    pt,
    ru,
    ja,
    ko,
    zh,
    nl,
    sv,
    pl,
    tr,
    ar,
  },
});

export default i18n;

// Helper to change language
export function setLanguage(lang: string) {
  if (isSupportedLocale(lang)) {
    i18n.global.locale.value = lang;
    localStorage.setItem('awawapp-language', lang);
  }
}

export function getCurrentLanguage(): string {
  return i18n.global.locale.value;
}

export { supportedLocales };
