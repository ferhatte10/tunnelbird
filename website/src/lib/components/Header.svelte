<script lang="ts">
	import { Check, Languages, Menu, X } from '@lucide/svelte';
	import { page } from '$app/state';
	import { m } from '#lib/paraglide/messages.js';
	import { deLocalizeHref, getLocale, locales, localizeHref } from '#lib/paraglide/runtime.js';
	import { LANGUAGE_NAMES, languageHref } from '#lib/language.js';
	import LanguageMenu from './LanguageMenu.svelte';
	import ThemeToggle from './ThemeToggle.svelte';

	const links = [
		{ href: '/install/', label: m.nav_install },
		{ href: '/docs/', label: m.nav_docs },
		{ href: '/privacy/', label: m.nav_privacy },
		{ href: '/changelog/', label: m.nav_changes }
	];

	const path = $derived(deLocalizeHref(page.url.pathname));

	// Phone menu: closed again after each navigation.
	let open = $state(false);
	$effect(() => {
		void page.url.pathname;
		open = false;
	});
</script>

<header class="container-site flex flex-wrap items-center gap-x-7 gap-y-3 py-4 md:py-5">
	<a
		href={localizeHref('/')}
		class="flex min-h-11 items-center gap-3 font-display text-[1.35rem] font-extrabold text-text no-underline hover:text-text"
	>
		<img src="/icon.svg" alt="" width="40" height="40" class="size-10" />
		Tunnelbird
	</a>

	<!-- Wide screens: the links in a row. -->
	<nav aria-label={m.nav_label()} class="ml-auto hidden items-center gap-x-6 text-[0.9rem] md:flex">
		{#each links as link (link.href)}
			{@const current = path === link.href}
			<a
				href={localizeHref(link.href)}
				aria-current={current ? 'page' : undefined}
				class="border-b-2 pb-0.5 no-underline {current
					? 'border-mark-red text-text'
					: 'border-transparent text-text-2 hover:text-text'}">{link.label()}</a
			>
		{/each}
		<LanguageMenu />
		<ThemeToggle />
	</nav>

	<!-- Phones: theme button and a menu that unfolds under the header, with the
	     languages at its end (a third button would not fit beside the name). -->
	<div class="ml-auto flex items-center gap-2 md:hidden">
		<ThemeToggle />
		<button
			type="button"
			aria-expanded={open}
			aria-controls="phone-menu"
			aria-label={open ? m.menu_close() : m.menu_open()}
			onclick={() => (open = !open)}
			class="inline-flex size-11 items-center justify-center rounded-xl bg-surface text-text"
		>
			{#if open}
				<X class="size-5" aria-hidden="true" />
			{:else}
				<Menu class="size-5" aria-hidden="true" />
			{/if}
		</button>
	</div>
	{#if open}
		<nav id="phone-menu" aria-label={m.nav_label()} class="w-full md:hidden">
			<ul class="m-0 flex list-none flex-col border-t border-border p-0">
				{#each links as link (link.href)}
					{@const current = path === link.href}
					<li class="border-b border-border">
						<a
							href={localizeHref(link.href)}
							aria-current={current ? 'page' : undefined}
							class="flex min-h-12 items-center gap-3 text-[1.05rem] no-underline {current
								? 'font-bold text-text'
								: 'text-text-2'}"
						>
							{#if current}<span class="h-5 w-1 rounded-full bg-mark-red" aria-hidden="true"></span>{/if}
							{link.label()}
						</a>
					</li>
				{/each}
			</ul>
			<p class="m-0 mt-4 flex items-center gap-2 text-[0.9rem] text-text-2">
				<Languages class="size-4" aria-hidden="true" />
				{m.language_label()}
			</p>
			<ul class="m-0 flex list-none flex-col p-0">
				{#each locales as locale (locale)}
					{@const current = locale === getLocale()}
					<li class="border-b border-border">
						<a
							href={languageHref(page.url, locale)}
							data-sveltekit-reload
							hreflang={locale}
							lang={locale}
							aria-current={current ? 'true' : undefined}
							class="flex min-h-12 items-center justify-between text-[1.05rem] no-underline {current
								? 'font-bold text-text'
								: 'text-text-2'}"
						>
							{LANGUAGE_NAMES[locale]}
							{#if current}<Check class="size-5 text-icon" aria-hidden="true" />{/if}
						</a>
					</li>
				{/each}
			</ul>
		</nav>
	{/if}
</header>
