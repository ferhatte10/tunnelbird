<script lang="ts">
	import { Download, FileCheck, Monitor, QrCode, TrendingUp } from '@lucide/svelte';
	import { m } from '#lib/paraglide/messages.js';
	import { localizeHref } from '#lib/paraglide/runtime.js';
	import { FLATHUB, INSTALL_COMMAND } from '#lib/links.js';
	import CommandBox from '#lib/components/CommandBox.svelte';
	import DemoVideo from '#lib/components/DemoVideo.svelte';
	import FlightLine from '#lib/components/FlightLine.svelte';
	import InstallTabs from '#lib/components/InstallTabs.svelte';
	import Screenshot from '#lib/components/Screenshot.svelte';
	import TriangleMark from '#lib/components/TriangleMark.svelte';
	import { reveal } from '#lib/reveal.js';

	const features = [
		{ icon: TrendingUp, title: m.f_traffic_title, text: m.f_traffic_text },
		{ icon: FileCheck, title: m.f_files_title, text: m.f_files_text },
		{ icon: Monitor, title: m.f_tray_title, text: m.f_tray_text },
		{ icon: QrCode, title: m.f_phone_title, text: m.f_phone_text }
	];
</script>

<svelte:head>
	<title>{m.home_title()}</title>
</svelte:head>

<section class="container-site flex flex-wrap items-center gap-x-14 gap-y-16 pt-10 pb-24 md:pt-16">
	<div class="flex min-w-0 flex-[1_1_32rem] flex-col gap-6">
		<h1 class="enter m-0 text-[clamp(2.6rem,7vw,4.25rem)] leading-[1.02] font-extrabold tracking-[-0.02em]">
			{m.hero_title_1()}<br />{m.hero_title_2()}
		</h1>
		<p class="enter m-0 max-w-[34ch] text-text-2 md:text-[1.1rem]" style="--d: 1">{m.hero_lead()}</p>
		<div class="enter" style="--d: 2">
			<CommandBox command={INSTALL_COMMAND} label={m.hero_install_label()} />
		</div>
		<div class="enter flex flex-wrap items-center gap-x-5 gap-y-3" style="--d: 3">
			<a
				href={FLATHUB}
				class="press inline-flex min-h-12 items-center gap-2.5 rounded-xl bg-surface px-5 font-bold text-text no-underline hover:text-link"
			>
				<Download class="size-5" aria-hidden="true" />
				{m.hero_flathub()}
			</a>
			<span class="text-[0.85rem] text-text-3"
				>{m.hero_other()} <a href={localizeHref('/install/')}>{m.hero_other_link()}</a></span
			>
		</div>
	</div>
	<div class="enter-settle relative min-w-0 flex-[1_1_24rem]" style="--d: 2">
		<FlightLine />
		<Screenshot name="main" alt={m.hero_screenshot_alt()} eager />
	</div>
</section>

<section class="container-site flex flex-col gap-5 pb-24">
	<h2
		use:reveal
		class="m-0 flex items-center gap-3.5 text-[clamp(1.9rem,4vw,2.5rem)] leading-tight font-extrabold"
	>
		<TriangleMark />{m.demo_title()}
	</h2>
	<p use:reveal style="--i: 1" class="m-0 max-w-[48ch] text-text-2">{m.demo_lead()}</p>
	<div use:reveal style="--i: 2" class="mt-3"><DemoVideo /></div>
</section>

<section id="install" class="border-y border-border bg-bg-alt">
	<div class="mx-auto flex max-w-[52rem] flex-col gap-5 px-5 py-20 md:px-8">
		<h2
			use:reveal
			class="m-0 flex items-center gap-3.5 text-[clamp(1.9rem,4vw,2.5rem)] leading-tight font-extrabold"
		>
			<TriangleMark />{m.install_title()}
		</h2>
		<div use:reveal style="--i: 1"><InstallTabs /></div>
	</div>
</section>

<section class="container-site flex flex-col gap-16 pt-24 pb-10">
	<h2
		use:reveal
		class="m-0 flex items-center gap-3.5 text-[clamp(1.9rem,4vw,2.5rem)] leading-tight font-extrabold"
	>
		<TriangleMark />{m.features_title()}
	</h2>
	<div class="flex flex-wrap-reverse items-center gap-14">
		<div use:reveal class="min-w-0 flex-[1_1_24rem]">
			<Screenshot name="trusted-networks" alt={m.wifi_alt()} class="max-w-[35rem]" />
		</div>
		<div use:reveal style="--i: 1" class="flex min-w-0 flex-[1_1_20rem] flex-col gap-3">
			<h3 class="m-0 text-[1.85rem] leading-tight font-semibold">{m.wifi_title()}</h3>
			<p class="m-0 text-text-2">{m.wifi_text()}</p>
		</div>
	</div>
</section>

<section class="container-site grid grid-cols-[repeat(auto-fit,minmax(14rem,1fr))] gap-x-12 gap-y-10 pt-10 pb-24">
	{#each features as feature, index (feature.title)}
		<div use:reveal style="--i: {index}" class="flex flex-col gap-2">
			<feature.icon class="size-7 text-icon" aria-hidden="true" />
			<h3 class="m-0 text-[1.35rem] font-semibold">{feature.title()}</h3>
			<p class="m-0 text-text-2">{feature.text()}</p>
		</div>
	{/each}
</section>

<section class="border-t border-border bg-bg-alt">
	<div class="container-site flex flex-wrap gap-12 py-20">
		<div use:reveal class="min-w-0 flex-[1_1_20rem]">
			<h2 class="m-0 mb-3 text-[1.85rem] font-extrabold">{m.private_title()}</h2>
			<p class="m-0 text-text-2">
				{m.private_text()} <a href={localizeHref('/privacy/')}>{m.private_link()}</a>.
			</p>
		</div>
		<div use:reveal style="--i: 1" class="min-w-0 flex-[1_1_20rem]">
			<h2 class="m-0 mb-3 text-[1.85rem] font-extrabold">{m.linux_title()}</h2>
			<p class="m-0 text-text-2">
				{m.linux_text()} <a href={localizeHref('/compatibility/')}>{m.linux_link()}</a>.
			</p>
		</div>
	</div>
</section>
