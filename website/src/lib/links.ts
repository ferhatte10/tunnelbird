// Addresses used across the site, in one place.

export const GITHUB = 'https://github.com/ferhatte10/tunnelbird';
export const RELEASES = `${GITHUB}/releases`;
export const ISSUES = `${GITHUB}/issues`;
export const INSTALL_SCRIPT_SOURCE = `${GITHUB}/blob/main/build-aux/install.sh`;
export const AUTHOR_NAME = 'Ferhat Saidoun';
export const AUTHOR_SITE = 'https://ferhatsn.fr';
export const FLATHUB = 'https://flathub.org/apps/fr.ferhatsn.Tunnelbird';
export const SNAP_STORE = 'https://snapcraft.io/tunnelbird';

export const INSTALL_COMMAND = 'curl -fsSL https://tunnelbird.ferhatsn.fr/install.sh | sh';

export type Method = 'apt' | 'dnf' | 'flatpak' | 'snap' | 'source' | 'windows' | 'macos';

/** The command of each way to install; none yet for Windows and macOS. */
export const COMMANDS: Record<Method, string | null> = {
	apt: INSTALL_COMMAND,
	dnf: INSTALL_COMMAND,
	flatpak: 'flatpak install flathub fr.ferhatsn.Tunnelbird',
	snap: 'sudo snap install tunnelbird',
	source: 'git clone https://github.com/ferhatte10/tunnelbird && cd tunnelbird && make && sudo make install',
	windows: null,
	macos: null
};
