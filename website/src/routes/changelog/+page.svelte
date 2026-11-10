<script lang="ts">
	import { m } from '#lib/paraglide/messages.js';
	import { getLocale } from '#lib/paraglide/runtime.js';
	import PageTitle from '#lib/components/PageTitle.svelte';
	// Built from the store listing's <releases> by scripts/assets.js.
	import releases from '#lib/generated/releases.json';

	function longDate(date: string): string {
		return new Date(`${date}T12:00:00Z`).toLocaleDateString(getLocale(), {
			year: 'numeric',
			month: 'long',
			day: 'numeric'
		});
	}

	type Item = { kind: string; text: string };
	type Block = { kind: 'p'; text: string } | { kind: 'ul'; items: string[] };

	/** Paragraphs as they are, and the items that follow each other in one list. */
	function blocks(items: Item[]): Block[] {
		const out: Block[] = [];
		for (const item of items) {
			const last = out.at(-1);
			if (item.kind !== 'li') out.push({ kind: 'p', text: item.text });
			else if (last?.kind === 'ul') last.items.push(item.text);
			else out.push({ kind: 'ul', items: [item.text] });
		}
		return out;
	}
</script>

<div class="mx-auto max-w-[46rem] px-5 pb-24 md:px-8">
	<PageTitle title={m.changes_title()} lead={m.changes_lead()} />
	{#each releases as release (release.version)}
		<article class="border-t border-border py-8">
			<h2 class="m-0 text-[1.6rem] font-extrabold">{m.changes_version({ version: release.version })}</h2>
			<p class="m-0 mb-4 text-[0.9rem] text-text-3"><time datetime={release.date}>{longDate(release.date)}</time></p>
			<div class="prose-site" lang="en">
				{#each blocks(release.items) as block, index (index)}
					{#if block.kind === 'ul'}
						<ul>
							{#each block.items as text, item (item)}
								<li>{text}</li>
							{/each}
						</ul>
					{:else}
						<p>{block.text}</p>
					{/if}
				{/each}
			</div>
		</article>
	{/each}
</div>
