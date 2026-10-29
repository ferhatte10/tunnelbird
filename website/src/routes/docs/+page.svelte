<script lang="ts">
	import { m } from '#lib/paraglide/messages.js';
	import { localizeHref } from '#lib/paraglide/runtime.js';
	import { ISSUES } from '#lib/links.js';
	import PageTitle from '#lib/components/PageTitle.svelte';

	const sections = [
		{ id: 'openvpn', title: m.docs_openvpn_title, text: m.docs_openvpn_text },
		{ id: 'trusted-networks', title: m.docs_trust_title, text: m.docs_trust_text },
		{ id: 'sharing', title: m.docs_share_title, text: m.docs_share_text },
		{ id: 'top-bar', title: m.docs_tray_title, text: m.docs_tray_text },
		{ id: 'updates', title: m.docs_updates_title, text: m.docs_updates_text }
	];
</script>

<div class="mx-auto max-w-[46rem] px-5 pb-24 md:px-8">
	<PageTitle title={m.docs_title()} lead={m.docs_lead()} />
	<nav aria-label={m.docs_title()} class="mb-4 flex flex-wrap gap-x-5 gap-y-2 text-[0.9rem]">
		<a href="#getting-started">{m.docs_start_title()}</a>
		{#each sections as section (section.id)}
			<a href="#{section.id}">{section.title()}</a>
		{/each}
		<a href="#troubleshooting">{m.docs_trouble_title()}</a>
	</nav>
	<div class="prose-site">
		<h2 id="getting-started">{m.docs_start_title()}</h2>
		<ol>
			<li>{m.docs_start_1()}</li>
			<li>{m.docs_start_2()}</li>
			<li>{m.docs_start_3()}</li>
		</ol>
		{#each sections as section (section.id)}
			<h2 id={section.id}>{section.title()}</h2>
			<p>{section.text()}</p>
		{/each}
		<h2 id="troubleshooting">{m.docs_trouble_title()}</h2>
		<ul>
			<li>{m.docs_trouble_server()}</li>
			<li>{m.docs_trouble_plugin()}</li>
			<li>
				{m.docs_trouble_snap()} (<a href={localizeHref('/install/') + '#steps'}>{m.install_links_steps()}</a>)
			</li>
			<li><a href={ISSUES}>{m.docs_trouble_more()}</a></li>
		</ul>
	</div>
</div>
