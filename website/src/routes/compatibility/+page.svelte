<script lang="ts">
	import { m } from '#lib/paraglide/messages.js';
	import PageTitle from '#lib/components/PageTitle.svelte';

	// Mirrors docs/COMPATIBILITY.md, the source of truth: keep both in step,
	// and write “Tested” only for what was really run.
	type Status = 'tested' | 'expected' | 'planned' | 'no';

	const status: Record<Status, { label: () => string; tone: string }> = {
		tested: { label: m.compat_tested, tone: 'bg-tab-on-bg text-tab-on-fg' },
		expected: { label: m.compat_expected, tone: 'bg-surface text-text' },
		planned: { label: m.compat_planned, tone: 'bg-surface text-text-2' },
		no: { label: m.compat_no, tone: 'bg-surface text-text-2' }
	};

	const cpus: { name: () => string; state: Status }[] = [
		{ name: m.compat_cpu_x86, state: 'tested' },
		{ name: m.compat_cpu_arm, state: 'expected' },
		{ name: m.compat_cpu_other, state: 'expected' }
	];

	const distros: { name: string; state: Status }[] = [
		{ name: 'Ubuntu 26.04 LTS', state: 'tested' },
		{ name: 'Ubuntu 24.04 LTS', state: 'expected' },
		{ name: 'Linux Mint 22, Pop!_OS 24.04, elementary OS 8, Zorin OS', state: 'expected' },
		{ name: 'Debian 13', state: 'expected' },
		{ name: 'Debian 12 (Flatpak)', state: 'expected' },
		{ name: 'Fedora 41+', state: 'expected' },
		{ name: 'openSUSE Tumbleweed, Arch, Manjaro (Flatpak or source)', state: 'expected' },
		{ name: 'RHEL, AlmaLinux, Rocky Linux 10 (Flatpak)', state: 'expected' },
		{ name: 'WSL, ChromeOS, systems without NetworkManager', state: 'no' }
	];

	const files: { name: () => string; state: Status }[] = [
		{ name: m.compat_files_wg, state: 'tested' },
		{ name: m.compat_files_ovpn, state: 'expected' },
		{ name: m.compat_files_no, state: 'no' }
	];
</script>

{#snippet badge(state: Status)}
	<span class="inline-flex flex-none items-center rounded-full px-3 py-0.5 text-[0.8rem] font-bold {status[state].tone}"
		>{status[state].label()}</span
	>
{/snippet}

{#snippet row(name: string, state: Status)}
	<li class="flex items-center justify-between gap-4 border-b border-border py-3 last:border-b-0">
		<span class="text-text-2">{name}</span>
		{@render badge(state)}
	</li>
{/snippet}

<div class="mx-auto max-w-[46rem] px-5 pb-24 md:px-8">
	<PageTitle title={m.compat_title()} lead={m.compat_lead()} />
	<p class="m-0 mb-10 text-[0.9rem] text-text-3">{m.compat_legend()}</p>

	<h2 class="mt-0 mb-2 text-[1.6rem] font-extrabold">{m.compat_cpu_title()}</h2>
	<ul class="m-0 list-none p-0">
		{#each cpus as cpu (cpu.name)}{@render row(cpu.name(), cpu.state)}{/each}
	</ul>

	<h2 class="mt-12 mb-2 text-[1.6rem] font-extrabold">{m.compat_distro_title()}</h2>
	<ul class="m-0 list-none p-0">
		{#each distros as distro (distro.name)}{@render row(distro.name, distro.state)}{/each}
	</ul>

	<h2 class="mt-12 mb-2 text-[1.6rem] font-extrabold">{m.compat_desktop_title()}</h2>
	<p class="m-0 text-text-2">{m.compat_desktop_text()}</p>

	<h2 class="mt-12 mb-2 text-[1.6rem] font-extrabold">{m.compat_files_title()}</h2>
	<ul class="m-0 list-none p-0">
		{#each files as file (file.name)}{@render row(file.name(), file.state)}{/each}
	</ul>

	<h2 class="mt-12 mb-2 flex items-center gap-3 text-[1.6rem] font-extrabold">
		{m.compat_other_title()}
		{@render badge('planned')}
	</h2>
	<p class="m-0 text-text-2">{m.compat_other_text()}</p>
</div>
