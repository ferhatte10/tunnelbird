// Addresses used across the site, in one place.

/**
 * While false (before a release), every install command and every GitHub link waits, and
 * the site says “coming soon”. It is true once the release exists, the install script and
 * the packages work, and the source repository is public: since 0.1.0 (2026-11-03).
 */
export const AVAILABLE = true;

/**
 * True once the signed APT and DNF repositories are published on this site. Until then
 * the script installs the self-contained package everywhere, and the pages say so.
 */
export const REPOSITORY = false;

export const GITHUB = 'https://github.com/ferhatte10/tunnelbird';
export const RELEASES = `${GITHUB}/releases`;
export const ISSUES = `${GITHUB}/issues`;
export const INSTALL_SCRIPT_SOURCE = `${GITHUB}/blob/main/build-aux/install.sh`;
export const AUTHOR_NAME = 'Ferhat Saidoun';
export const AUTHOR_SITE = 'https://ferhatsn.fr';

export const INSTALL_COMMAND = 'curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh';

export type Method = 'linux' | 'macos' | 'windows' | 'source';

/** The command of each way to install; none yet for Windows. */
export const COMMANDS: Record<Method, string | null> = {
	linux: INSTALL_COMMAND,
	macos: INSTALL_COMMAND,
	source: 'git clone https://github.com/ferhatte10/tunnelbird && cd tunnelbird && make && sudo make install',
	windows: null
};
