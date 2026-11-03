// Addresses used across the site, in one place.

/**
 * False until the first release: the install script, the packages and signed
 * repositories exist, and the source repository is public. Until then every install
 * command and every GitHub link waits, and the site says “coming soon”.
 */
export const AVAILABLE = false;

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
