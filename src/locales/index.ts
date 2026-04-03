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

// Get saved language or detect from browser
function getDefaultLocale(): string {
  // Check localStorage first
  const saved = localStorage.getItem('awawapp-language');
  if (saved && supportedLocales.includes(saved)) {
    return saved;
  }
  
  // Detect from browser
  const browserLang = navigator.language.split('-')[0];
  if (supportedLocales.includes(browserLang)) {
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
  if (supportedLocales.includes(lang)) {
    i18n.global.locale.value = lang as typeof supportedLocales[number];
    localStorage.setItem('awawapp-language', lang);
  }
}

export function getCurrentLanguage(): string {
  return i18n.global.locale.value;
}

export { supportedLocales };
