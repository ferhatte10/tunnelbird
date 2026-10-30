<script lang="ts">
	import { getLocale } from '#lib/paraglide/runtime.js';

	// A store screenshot in the visitor's language and theme: `<name>.png` and
	// `<name>-dark.png`, the one not matching the theme hidden. `eager` loads
	// both at once (the hero): the hidden one would else arrive late after a
	// theme switch, or on load in dark mode.
	let {
		name,
		alt,
		eager = false,
		class: className = ''
	}: { name: string; alt: string; eager?: boolean; class?: string } = $props();

	const base = $derived(`/screenshots/${getLocale()}`);
	const loading = $derived(eager ? 'eager' : 'lazy');
</script>

<img
	src="{base}/{name}.png"
	{alt}
	width="1412"
	height="1500"
	{loading}
	class="block h-auto w-full dark:hidden {className}"
/>
<img
	src="{base}/{name}-dark.png"
	{alt}
	width="1412"
	height="1500"
	{loading}
	class="hidden h-auto w-full dark:block {className}"
/>
