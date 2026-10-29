# tunnelbird.ferhatsn.fr

The project's website: presentation, installation, documentation, privacy,
compatibility, changelog. English at `/`, French at `/fr/`. Design rules:
[DESIGN.md](DESIGN.md); plan and deployment: `../docs/plans/website.md`.

A static site (SvelteKit with `adapter-static`): every page is a plain file,
nothing runs on the server, no cookie, no analytics, no third-party request.

```sh
bun install
bun run dev       # http://localhost:5173
bun run check     # types + contrast of every colour pair, both themes
bun run build     # → build/, served as is by nginx
```

`scripts/assets.js` (run by dev, check and build) copies from the app's own
sources what the site shows: the icon, the store screenshots, `install.sh`,
and the release notes of the store listing (for “What's new”). Those copies
are not committed.

Keep the compatibility page in step with `../docs/COMPATIBILITY.md`, the
source of truth.
