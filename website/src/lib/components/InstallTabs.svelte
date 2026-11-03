<script lang="ts">
	import { Tabs } from 'bits-ui';
	import { m } from '#lib/paraglide/messages.js';
	import { localizeHref } from '#lib/paraglide/runtime.js';
	import { AVAILABLE, COMMANDS, GITHUB, INSTALL_SCRIPT_SOURCE, REPOSITORY, type Method } from '#lib/links.js';
	import CommandBox from './CommandBox.svelte';

	const methods: { id: Method; label: () => string; text: () => string }[] = [
		{ id: 'linux', label: m.tab_linux, text: REPOSITORY ? m.tab_linux_text : m.tab_linux_text_package },
		{ id: 'macos', label: m.tab_macos, text: m.tab_macos_text },
		{ id: 'windows', label: m.tab_windows, text: m.tab_windows_text },
		{ id: 'source', label: m.tab_source, text: m.tab_source_text }
	];

	let selected = $state<string>('linux');
</script>

<Tabs.Root bind:value={selected} class="flex flex-col gap-5">
	<Tabs.List aria-label={m.install_systems()} class="flex flex-wrap gap-2">
		{#each methods as method (method.id)}
			<Tabs.Trigger
				value={method.id}
				class="press min-h-11 rounded-full bg-surface px-[1.1rem] text-[0.9rem] font-bold text-text hover:text-link data-[state=active]:bg-tab-on-bg data-[state=active]:text-tab-on-fg"
			>
				{method.label()}
			</Tabs.Trigger>
		{/each}
	</Tabs.List>
	{#each methods as method (method.id)}
		{@const command = COMMANDS[method.id]}
		<Tabs.Content value={method.id} class="tab-panel flex flex-col gap-4">
			{#if command && AVAILABLE}
				<p class="m-0 text-text-2">{method.text()}</p>
				<CommandBox {command} />
			{:else}
				<p class="m-0 font-display text-xl font-semibold">{m.tab_soon_title()}</p>
				<p class="m-0 text-text-2">{method.text()}</p>
				{#if AVAILABLE}<p class="m-0"><a href={GITHUB}>{m.tab_soon_follow()}</a></p>{/if}
			{/if}
		</Tabs.Content>
	{/each}
	<p class="m-0 text-[0.85rem] text-text-3">
		{#if AVAILABLE}
			<a href={INSTALL_SCRIPT_SOURCE}>{m.install_links_script()}</a> ·
			<a href={localizeHref('/install/') + '#steps'}>{m.install_links_steps()}</a> ·
		{/if}
		<a href={localizeHref('/compatibility/')}>{m.install_links_compat()}</a>
	</p>
</Tabs.Root>
