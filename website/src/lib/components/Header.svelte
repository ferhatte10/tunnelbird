<script lang="ts">
	import { Menu, X } from '@lucide/svelte';
	import { page } from '$app/state';
	import { m } from '#lib/paraglide/messages.js';
	import { deLocalizeHref, getLocale, localizeHref } from '#lib/paraglide/runtime.js';
	import ThemeToggle from './ThemeToggle.svelte';

	const links = [
		{ href: '/install/', label: m.nav_install },
		{ href: '/docs/', label: m.nav_docs },
		{ href: '/privacy/', label: m.nav_privacy },
		{ href: '/changelog/', label: m.nav_changes }
	];

	const path = $derived(deLocalizeHref(page.url.pathname));
	const otherLocale = $derived(getLocale() === 'fr' ? 'en' : 'fr');

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
		<a
			href={localizeHref(path, { locale: otherLocale })}
			hreflang={otherLocale}
			lang={otherLocale}
			class="text-text-2 no-underline hover:text-text">{m.other_language()}</a
		>
		<ThemeToggle />
	</nav>

	<!-- Phones: theme button and a menu that unfolds under the header. -->
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
				<li>
					<a
						href={localizeHref(path, { locale: otherLocale })}
						hreflang={otherLocale}
						lang={otherLocale}
						class="flex min-h-12 items-center text-[1.05rem] text-text-2 no-underline">{m.other_language()}</a
					>
				</li>
			</ul>
		</nav>
	{/if}
</header>
