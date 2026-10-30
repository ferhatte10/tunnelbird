<script lang="ts">
	import { onMount } from 'svelte';
	import { prefersReducedMotion } from 'svelte/motion';
	import { MediaQuery } from 'svelte/reactivity';
	import { Pause, Play } from '@lucide/svelte';
	import { m } from '#lib/paraglide/messages.js';
	import { getLocale } from '#lib/paraglide/runtime.js';
	import { versioned } from '#lib/assets.js';

	// The 21 s film of the app (brag-output, rendered with HyperFrames), in the
	// visitor's language and theme, muted, looping; in portrait (4:5) on phones,
	// where the 16:9 film would be too small to read. Costs nothing until it is
	// useful (DESIGN.md, Motion): the page only carries the poster (lazy); the
	// <video> exists in the browser only, `preload="none"`, and plays on hover or
	// focus with a mouse, when 60 % visible on a touch screen, never with
	// reduced motion or data saving (the poster then stays). A button pauses or
	// plays it at any time (WCAG 2.2.2: motion longer than 5 s can be stopped).

	const base = `/videos/${getLocale()}`;
	const hover = new MediaQuery('(hover: hover)');
	// Same breakpoint as the layout (md, 48rem) and the <source> of the posters.
	const PHONE = '(max-width: 47.99rem)';
	const phone = new MediaQuery(PHONE);

	let root = $state<HTMLElement>();
	let video = $state<HTMLVideoElement>();
	let mounted = $state(false);
	let saveData = $state(false);
	let dark = $state(false);
	let active = $state(false);
	let visible = $state(false);
	// The visitor's own choice from the button, over hover and visibility.
	let choice = $state<'play' | 'pause' | null>(null);
	let playing = $state(false);

	onMount(() => {
		mounted = true;
		saveData = !!(navigator as Navigator & { connection?: { saveData?: boolean } }).connection
			?.saveData;
		// The film follows the theme, also when it changes on the page.
		const read = () => (dark = document.documentElement.dataset.theme === 'dark');
		read();
		const observer = new MutationObserver(read);
		observer.observe(document.documentElement, { attributeFilter: ['data-theme'] });
		return () => observer.disconnect();
	});

	const canPlay = $derived(mounted && !prefersReducedMotion.current && !saveData);
	const src = $derived(
		versioned(`${base}-${dark ? 'dark' : 'light'}${phone.current ? '-portrait' : ''}.mp4`)
	);
	const shouldPlay = $derived(
		canPlay && (choice ? choice === 'play' : hover.current ? active : visible)
	);

	// Touch screens: play while most of the film is on screen.
	$effect(() => {
		if (!canPlay || hover.current || !root) return;
		const observer = new IntersectionObserver(([entry]) => (visible = !!entry?.isIntersecting), {
			threshold: 0.6
		});
		observer.observe(root);
		return () => observer.disconnect();
	});

	$effect(() => {
		if (!video) return;
		void src;
		if (shouldPlay) {
			// Refused (power saving…): the poster stays.
			video.play().catch(() => (playing = false));
		} else {
			video.pause();
			playing = false;
		}
	});
</script>

<div
	bind:this={root}
	role="group"
	aria-label={m.demo_label()}
	onpointerenter={() => (active = true)}
	onpointerleave={() => (active = false)}
	onfocusin={() => (active = true)}
	onfocusout={() => (active = false)}
	class="relative mx-auto aspect-[4/5] max-w-[30rem] overflow-hidden rounded-2xl border border-border bg-bg-alt md:aspect-video md:max-w-none"
>
	<!-- Poster: the connected app, in each theme and format (visible without
	     JavaScript; the browser fetches only the matching one). -->
	{#each [{ theme: 'light', cls: 'dark:hidden' }, { theme: 'dark', cls: 'hidden dark:block' }] as poster (poster.theme)}
		<picture>
			<source
				media={PHONE}
				srcset={versioned(`${base}-${poster.theme}-portrait.jpg`)}
				width="864"
				height="1080"
			/>
			<img
				src={versioned(`${base}-${poster.theme}.jpg`)}
				alt={m.demo_alt()}
				width="1280"
				height="720"
				loading="lazy"
				decoding="async"
				class="absolute inset-0 size-full object-cover {poster.cls}"
			/>
		</picture>
	{/each}

	{#if canPlay}
		{#key src}
			<video
				bind:this={video}
				{src}
				muted
				loop
				playsinline
				preload="none"
				disableremoteplayback
				aria-hidden="true"
				tabindex="-1"
				onplaying={() => (playing = true)}
				class="absolute inset-0 size-full object-cover transition-opacity duration-300 {playing
					? 'opacity-100'
					: 'opacity-0'}"
			></video>
		{/key}
		<button
			type="button"
			onclick={() => (choice = shouldPlay ? 'pause' : 'play')}
			class="press absolute right-3 bottom-3 inline-flex size-11 items-center justify-center rounded-full bg-bg-alt/90 text-text shadow-md hover:text-link"
		>
			{#if shouldPlay}
				<Pause class="size-5" aria-hidden="true" />
				<span class="sr-only">{m.demo_pause()}</span>
			{:else}
				<Play class="size-5" aria-hidden="true" />
				<span class="sr-only">{m.demo_play()}</span>
			{/if}
		</button>
	{/if}
</div>
