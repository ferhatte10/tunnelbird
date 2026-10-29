// Copies what the site shows from the app's own sources, so nothing is kept
// twice: the icon, the store screenshots, install.sh, and the release notes of
// the store listing (for “What's new”). Runs before dev, build and check.
// The copies are not committed (see .gitignore).

import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const site = join(dirname(fileURLToPath(import.meta.url)), '..');
const repo = join(site, '..');

function copy(from, to) {
	mkdirSync(dirname(to), { recursive: true });
	copyFileSync(from, to);
}

copy(
	join(repo, 'data/icons/hicolor/scalable/apps/fr.ferhatsn.Tunnelbird.svg'),
	join(site, 'static/icon.svg')
);
copy(join(repo, 'build-aux/install.sh'), join(site, 'static/install.sh'));

for (const lang of ['en', 'fr']) {
	const from = join(repo, 'data/screenshots', lang);
	for (const file of readdirSync(from)) {
		copy(join(from, file), join(site, 'static/screenshots', lang, file));
	}
}

// Releases from the metainfo: version, date and notes (AppStream markup,
// <p> and <ul><li> only, written by us).
const metainfo = readFileSync(join(repo, 'data/fr.ferhatsn.Tunnelbird.metainfo.xml'), 'utf8');
const releases = [...metainfo.matchAll(/<release version="([^"]+)" date="([^"]+)">([\s\S]*?)<\/release>/g)].map(
	([, version, date, body]) => {
		const description = body.match(/<description>([\s\S]*?)<\/description>/)?.[1] ?? '';
		const items = [...description.matchAll(/<(p|li)>([\s\S]*?)<\/\1>/g)].map(([, kind, text]) => ({
			kind,
			text: text.replace(/\s+/g, ' ').trim()
		}));
		return { version, date, items };
	}
);
mkdirSync(join(site, 'src/lib/generated'), { recursive: true });
writeFileSync(join(site, 'src/lib/generated/releases.json'), JSON.stringify(releases, null, '\t') + '\n');
