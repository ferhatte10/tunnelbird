import { deLocalizeHref, localizeHref, type Locale } from '#lib/paraglide/runtime.js';

/**
 * A language under its own name ("English", "Français", "Deutsch"), found
 * whatever the page's language. From the browser's own data: a new language
 * needs no label here.
 */
export function nativeName(locale: Locale): string {
	const name = new Intl.DisplayNames([locale], { type: 'language' }).of(locale) ?? locale;
	return name.charAt(0).toLocaleUpperCase(locale) + name.slice(1);
}

/** Up to this many languages, phones show them side by side; beyond, a list. */
export const SIDE_BY_SIDE = 3;

/**
 * The same page in another language. Always open it as a new document
 * (`data-sveltekit-reload`, or `location.assign`): Paraglide's messages and
 * `<html lang>` only follow the language on a full page load, so a
 * client-side navigation would keep the old texts until a refresh.
 */
export function languageHref(url: Pick<URL, 'pathname' | 'hash'>, locale: Locale): string {
	return localizeHref(deLocalizeHref(url.pathname), { locale }) + url.hash;
}
