<script lang="ts">
	import { m } from '#lib/paraglide/messages.js';
	import { AVAILABLE, RELEASES } from '#lib/links.js';
	import CommandBox from '#lib/components/CommandBox.svelte';
	import InstallTabs from '#lib/components/InstallTabs.svelte';
	import PageTitle from '#lib/components/PageTitle.svelte';

	const apt = `sudo install -d -m 755 /etc/apt/keyrings
curl -fsSL https://tunnelbird.ferhatsn.fr/tunnelbird.gpg | sudo tee /etc/apt/keyrings/tunnelbird.gpg > /dev/null
printf 'Types: deb\\nURIs: https://tunnelbird.ferhatsn.fr/apt\\nSuites: stable\\nComponents: main\\nSigned-By: /etc/apt/keyrings/tunnelbird.gpg\\n' | sudo tee /etc/apt/sources.list.d/tunnelbird.sources
sudo apt update && sudo apt install tunnelbird`;

	const dnf = `printf '[tunnelbird]\\nname=Tunnelbird\\nbaseurl=https://tunnelbird.ferhatsn.fr/rpm/$basearch\\nenabled=1\\ngpgcheck=1\\nrepo_gpgcheck=1\\ngpgkey=https://tunnelbird.ferhatsn.fr/tunnelbird.gpg\\n' | sudo tee /etc/yum.repos.d/tunnelbird.repo
sudo dnf install tunnelbird`;

	const macosUninstall = `sudo "/Library/Application Support/Tunnelbird/tunnelbird-service" --uninstall-daemon
sudo rm -rf "/Library/Application Support/Tunnelbird"
rm -rf /Applications/Tunnelbird.app`;

	const sourceApt = 'sudo apt install libgtk-4-dev libadwaita-1-dev libsoup-3.0-dev gettext desktop-file-utils';
	const sourceDnf = 'sudo dnf install gtk4-devel libadwaita-devel libsoup3-devel gettext desktop-file-utils';

	const uninstall = [
		{ label: m.tab_apt, command: 'sudo apt remove tunnelbird' },
		{ label: m.tab_dnf, command: 'sudo dnf remove tunnelbird' },
		{ label: m.uninstall_script, command: 'rm -rf ~/.local/share/tunnelbird ~/.local/bin/tunnelbird' },
		{ label: m.tab_macos, command: macosUninstall },
		{ label: m.tab_source, command: 'sudo make uninstall' }
	];
</script>

<div class="mx-auto max-w-[52rem] px-5 pb-24 md:px-8">
	<PageTitle
		title={m.install_page_title()}
		lead={AVAILABLE ? m.install_page_lead() : m.install_soon_lead()}
		tab={m.nav_install()}
	/>
	<InstallTabs />

	{#if AVAILABLE}
	<h2 id="steps" class="mt-16 mb-2 text-[1.75rem] font-extrabold">{m.steps_title()}</h2>
	<p class="m-0 mb-6 text-text-2">{m.steps_lead()}</p>
	<div class="flex flex-col gap-6">
		<CommandBox command={apt} label={m.steps_apt()} />
		<CommandBox command={dnf} label={m.steps_dnf()} />
		<CommandBox command={sourceApt} label="{m.steps_source_deps()} · {m.tab_apt()}" />
		<CommandBox command={sourceDnf} label="{m.steps_source_deps()} · {m.tab_dnf()}" />
	</div>

	{/if}

	<h2 class="mt-16 mb-2 text-[1.75rem] font-extrabold">{m.requirements_title()}</h2>
	<p class="m-0 text-text-2">{m.requirements_text()}</p>

	{#if AVAILABLE}
	<h2 class="mt-16 mb-2 text-[1.75rem] font-extrabold">{m.download_title()}</h2>
	<p class="m-0 text-text-2">{m.download_text()} <a href={RELEASES}>{m.download_link()}</a>.</p>

	<h2 class="mt-16 mb-2 text-[1.75rem] font-extrabold">{m.uninstall_title()}</h2>
	<p class="m-0 mb-6 text-text-2">{m.uninstall_text()}</p>
	<div class="flex flex-col gap-5">
		{#each uninstall as item (item.command)}
			<CommandBox command={item.command} label={item.label()} />
		{/each}
	</div>
	{/if}
</div>
