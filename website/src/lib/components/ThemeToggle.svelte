<script lang="ts">
	import { onMount } from 'svelte';
	import { Moon, Sun } from '@lucide/svelte';
	import { m } from '#lib/paraglide/messages.js';

	// Set by /theme.js before the first paint; read once mounted.
	let dark = $state(false);

	onMount(() => {
		dark = document.documentElement.dataset.theme === 'dark';
	});

	function toggle() {
		dark = !dark;
		document.documentElement.dataset.theme = dark ? 'dark' : 'light';
		try {
			localStorage.setItem('theme', dark ? 'dark' : 'light');
		} catch {
			// Storage refused (private browsing): the choice lasts for this page.
		}
	}
</script>

<button
	type="button"
	onclick={toggle}
	aria-label={dark ? m.theme_to_light() : m.theme_to_dark()}
	class="inline-flex size-11 items-center justify-center rounded-xl bg-surface text-text transition-colors hover:text-link"
>
	{#if dark}
		<Sun class="size-5" aria-hidden="true" />
	{:else}
		<Moon class="size-5" aria-hidden="true" />
	{/if}
</button>
