import enUS from './en-US.mjs';
import frFR from './fr-FR.mjs';

export const defaultLocale = 'en-US';
export const sourceLocale = 'fr-FR';
export const languages = {
  'en-US': { label: 'English (US)', messages: enUS },
  'fr-FR': { label: 'Français (France)', messages: frFR },
};
