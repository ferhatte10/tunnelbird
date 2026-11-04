/** The visitor's system, to offer the install command that suits it first. */
export type System = 'linux' | 'macos' | 'windows' | 'other';

/**
 * Best guess from the browser, never a promise: an iPad may call itself a Mac, a browser may hide
 * its platform. Before the page runs this (prerendering, no script) the site shows Linux.
 */
export function detectSystem(): System {
	if (typeof navigator === 'undefined') return 'linux';
	const agent = navigator.userAgent;
	const platform =
		(navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ?? '';
	if (/android|iphone|ipad|ipod/i.test(agent)) return 'other';
	if (/^mac/i.test(platform) || /macintosh|mac os x/i.test(agent)) {
		// An iPad that asks for the desktop site has a touch screen; no Mac has one.
		return navigator.maxTouchPoints > 1 ? 'other' : 'macos';
	}
	if (/^win/i.test(platform) || /windows/i.test(agent)) return 'windows';
	return 'linux';
}
