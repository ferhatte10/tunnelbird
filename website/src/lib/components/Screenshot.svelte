<script lang="ts">
	import { getLocale } from '#lib/paraglide/runtime.js';

	// A store screenshot in the visitor's language; `dark` names the variant
	// shown with the dark theme, when there is one.
	let {
		name,
		dark,
		alt,
		eager = false,
		class: className = ''
	}: { name: string; dark?: string; alt: string; eager?: boolean; class?: string } = $props();

	const base = $derived(`/screenshots/${getLocale()}`);
</script>

<img
	src="{base}/{name}.png"
	{alt}
	width="1412"
	height="1500"
	loading={eager ? 'eager' : 'lazy'}
	class="block h-auto w-full {dark ? 'dark:hidden' : ''} {className}"
/>
{#if dark}
	<img
		src="{base}/{dark}.png"
		{alt}
		width="1412"
		height="1500"
		loading="lazy"
		class="hidden h-auto w-full dark:block {className}"
	/>
{/if}
