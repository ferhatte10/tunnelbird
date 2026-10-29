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

## Container image

What the cluster runs: the built pages in an unprivileged nginx (uid 101,
port 8080, read-only, only `/tmp` written). Built from the repository root,
because the site copies the app's files at build time:

```sh
docker build -f website/Dockerfile -t tunnelbird-website .
docker run --rm -p 8080:8080 --read-only --tmpfs /tmp --cap-drop ALL tunnelbird-website
```

`nginx/default.conf` sets the caching (pages always checked again, hashed
files for a year) and `nginx/security-headers.conf` the headers sent with
every answer. `.github/workflows/website.yml` builds, scans (Trivy), serves and
tests this image on each pull request, and publishes it to GHCR from `main`.

Keep the compatibility page in step with `../docs/COMPATIBILITY.md`, the
source of truth.
