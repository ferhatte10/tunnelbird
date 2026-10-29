<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { m } from '#lib/paraglide/messages.js';
	import { deLocalizeHref, localizeHref, locales } from '#lib/paraglide/runtime.js';
	import Header from '#lib/components/Header.svelte';
	import Footer from '#lib/components/Footer.svelte';

	let { children } = $props();

	const origin = 'https://tunnelbird.ferhatsn.fr';
	const path = $derived(deLocalizeHref(page.url.pathname));
</script>

<svelte:head>
	<meta name="description" content={m.site_description()} />
	<link rel="canonical" href={origin + localizeHref(path)} />
	{#each locales as locale (locale)}
		<link rel="alternate" hreflang={locale} href={origin + localizeHref(path, { locale })} />
	{/each}
	<meta property="og:site_name" content="Tunnelbird" />
	<meta property="og:image" content={origin + '/screenshots/en/main.png'} />
</svelte:head>

<a
	href="#content"
	class="sr-only z-10 rounded-lg bg-accent px-4 py-2 text-on-accent focus:not-sr-only focus:absolute focus:top-3 focus:left-3"
	>{m.skip_to_content()}</a
>
<Header />
<main id="content" class="flex-1">
	{@render children()}
</main>
<Footer />
