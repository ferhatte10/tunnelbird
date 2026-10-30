import { deLocalizeHref, localizeHref, type Locale } from '#lib/paraglide/runtime.js';

/** Each language under its own name: found whatever the page's language. */
export const LANGUAGE_NAMES: Record<Locale, string> = { en: 'English', fr: 'Français' };

/**
 * The same page in another language. Always open it as a new document
 * (`data-sveltekit-reload`, or `location.assign`): Paraglide's messages and
 * `<html lang>` only follow the language on a full page load, so a
 * client-side navigation would keep the old texts until a refresh.
 */
export function languageHref(url: Pick<URL, 'pathname' | 'hash'>, locale: Locale): string {
	return localizeHref(deLocalizeHref(url.pathname), { locale }) + url.hash;
}
