# Plan: the website, `tunnelbird.ferhatsn.fr`

Status: **site written** (`website/`, 2026-10-30) in the validated A+B
direction (see `website/DESIGN.md`). **Deployment prepared** (2026-10-29):
container image, workflow, cluster manifests on the `feat/tunnelbird-site`
branch of `ferhat-k8s`; not deployed yet (first image to publish). Still
open below: the package-signing CronJob, analytics (none so far).

## What the site is for

1. Make someone want Tunnelbird in ten seconds: what it does, how it looks.
2. Get it installed in one minute, whatever the distribution.
3. Answer questions without opening an issue (docs, privacy, FAQ).
4. Serve what the app and the installer need: `install.sh`, the APT/DNF
   repositories, the signing key and `latest.json`.

## Stack (same as the portfolio, so one way of working)

- SvelteKit + Svelte 5, **static build** (`adapter-static`, every page
  prerendered): no server code, nothing to attack, cached by the browser.
- Tailwind 4, shadcn-svelte / bits-ui, Lucide icons, Paraglide for English and
  French (English at `/`, French at `/fr/…`, language picker remembered in a
  cookie, `Accept-Language` only as a first suggestion).
- Bun for scripts and builds; Vitest (unit, contrast tests), Playwright + axe
  (end-to-end, WCAG 2.2 AA), Lighthouse on the production build.
- Served by an unprivileged nginx image (static files), two replicas.

**Decision**: the code lives in `website/` inside this repository (shares the
screenshots, the metainfo release notes and the icon with the app; one pull
request can change the app and its docs) or in its own repository.
Recommendation: `website/` here.

## Identity

From the app, nothing invented:

- The E3 icon as-is (never redrawn), night background `#222a3b`, the red
  `#e23439` and blue `#296eb4` of the triangle, the swallow as the single
  decorative motif (a line drawing in the hero, the 404 page, the share image).
- Blue = interactive (links, focus, main button); red = landmark (active tab,
  “connected” state in illustrations), like the portfolio.
- Day and night themes, following the system, with a toggle.
- Real screenshots of the app (the demo-mode set from `data/screenshots/`),
  both themes, never mock-ups drawn in HTML.

Process (from the UI-quality rules): run `ui-ux-pro-max` to propose two or
three distinct directions, the owner picks one, it is written down in
`website/DESIGN.md` before the first page. None of the refused “AI template”
habits: no pulsing status dot, no spaced uppercase mono overlines, no row of
key figures, no blurred halos or background grid, no bordered cards
everywhere. Pointer cursor on everything clickable, set globally from day one.
Busy button = spinner and changed label.

## Pages

| Page | Content |
|---|---|
| Home | Name, one sentence, the main screenshot (theme follows the visitor), “Install” button detecting the distribution from the user agent when possible, five strengths with a screenshot each (import + preview, live graph and location, trusted networks, top bar, QR sharing), “Free and open source” with the GitHub link |
| Install | Tabs: Linux (one command for every distribution), macOS, Windows (“coming soon”), build from source. Each tab: the one-line command with a copy button, then the steps it runs, readable, so nothing is hidden. Link to `install.sh` source. Requirements (NetworkManager, Linux 5.6+) |
| Docs | Getting started (import a file, preview, connect), trusted networks and the strict check, sharing (file, QR) with the private-key warning, top bar and background mode, updates per install method, troubleshooting (no top bar icon on GNOME without the extension, “server does not answer”), FAQ |
| Privacy | What leaves the computer: the public-IP lookup (ifconfig.co) when connected, the daily `latest.json` check (can be turned off), nothing else; no account, no telemetry. The site itself: no cookies but the language and theme choices, no analytics (**Decision**: or the same self-hosted analytics as the portfolio) |
| Changelog | Generated at build time from the metainfo `<releases>` (one source of truth) |
| Download | Direct `.deb` / `.rpm` links of the latest release, with SHA-256 and the `gh attestation verify` command |
| 404 | Same identity, links to Home and Install |

Every page: English and French, no hard-coded text, empty/error/loading states,
phone and desktop, day and night. Open Graph images in both languages.

## Served files

| Path | Source |
|---|---|
| `/install.sh` | `build-aux/install.sh` (written and checked, copied by the site build) |
| `/latest.json` | The latest GitHub release asset (see below) |
| `/apt/…`, `/rpm/…` | Signed repositories (see below) |
| `/tunnelbird.gpg` | Public signing key |

## `install.sh`

- POSIX `sh`, `set -eu`, every step printed before it runs, `sudo` only for the
  steps that need it and announced first, `--dry-run` to only print.
- Reads `/etc/os-release`: Debian family → add the key and
  `deb [signed-by=…] https://tunnelbird.ferhatsn.fr/apt stable main`, then
  `apt install tunnelbird`; Fedora family → `/etc/yum.repos.d/tunnelbird.repo`
  with `gpgcheck=1`, then `dnf install tunnelbird`; anything else → the self-contained package
  (GTK and libadwaita inside, installed in the home folder; planned). On macOS
  the same command installs the app (planned).
- Checks NetworkManager is running and warns otherwise.
- Tested in CI (or locally with Podman) in Ubuntu 24.04, Debian 13,
  Fedora 42 containers with `--dry-run`, plus one real install per family.

## Package repositories and signing

The rule is “no secret in CI”, so signing happens in the cluster:

- A **CronJob** (every 30 minutes) in the `tunnelbird` namespace:
  1. asks the GitHub API for the latest release; stops if already published;
  2. downloads the `.deb`, `.rpm` and `latest.json`, checks `SHA256SUMS` and the
     build attestation;
  3. rebuilds the APT repository (`reprepro`) and the DNF repository
     (`createrepo_c`), signs them with the GPG key from a SOPS secret
     (KSOPS, like every other secret of the cluster);
  4. writes everything to a volume the nginx pods mount read-only.
- So a release needs no manual step: tag → CI → within 30 minutes, every APT/DNF
  user sees the update in their system updates, and the app sees `latest.json`.
- The GPG key is created once, offline, by the owner; its public half is
  `/tunnelbird.gpg` and goes into the README.

**Decision**: this CronJob and its volume are new components (storage class to
choose: the NFS share or local). Alternative without anything new: GitHub
Pages hosting the repositories (but then the key would have to be a CI secret,
which the rules forbid).

Where the CronJob writes (both pods must read it, so not a local disk):

| Option | New in the cluster | For | Against |
|---|---|---|---|
| **S3 bucket `tunnelbird` on Garage** (recommended) | a bucket and a key, like every project | nothing to mount: nginx proxies `/apt/`, `/rpm/`, `/tunnelbird.gpg`, `/latest.json` to the bucket; the site pods stay read-only and stateless | one more `location` per path in nginx |
| Volume on the NAS (csi-driver-nfs, already there) | a PersistentVolume | plain files, simplest CronJob | the site's pods then wait for the NAS at start-up, like the media apps |

Either way: the CronJob image (`reprepro`, `createrepo_c`, `gpg`) pinned by
digest, the GPG key in a SOPS secret, and the key created by the owner.

## Deployment (like the portfolio)

- **Image** (`website/Dockerfile`): the static build in
  `nginx-unprivileged` (alpine), both base images pinned by digest, security
  updates applied, curl removed. Read-only root filesystem, uid 101, port 8080.
  Caching and security headers in `website/nginx/` (HSTS, nosniff, referrer
  and permissions policies, `frame-ancestors 'none'`, COOP; the rest of the
  CSP is in each page, by hash). Checked: Trivy HIGH/CRITICAL clean.
- **Workflow** (`.github/workflows/website.yml`): on pull requests touching
  the site, build (type check, contrast test, build), Trivy scan (pinned
  binary, blocking), serve the image read-only and run smoke tests (every
  page, the 404, the relative redirect, `install.sh` as text, the headers).
  On `main`, the same image is pushed to GHCR as `latest` and `<sha>`.
- **Cluster** (`ferhat-k8s`, `apps/tunnelbird/` and `argocd/apps/tunnelbird.yaml`,
  project `sites`): namespace (restricted pod security), Deployment (2
  replicas spread over the nodes, non-root, read-only, no service account
  token, 16 MiB requested, 64 MiB limit), Service, HTTPRoute on the Traefik
  Gateway with the `*.ferhatsn.fr` certificate, PodDisruptionBudget, GHCR pull
  secret (SOPS). Image Updater pins the digest of each new `latest`. The site
  also appears in the Traefik dashboard and the alerts under “Tunnelbird
  (site)”.
- **DNS**: nothing to do, the `*.ferhatsn.fr` record already sends
  `tunnelbird.ferhatsn.fr` to the home entry point.
- CrowdSec already filters in front of Traefik.
- Same rule as the portfolio: nothing on the site reveals the hosting
  (no host names, IPs, versions).

### First deployment

Hosted runners cost minutes while the repository is private, so the first
image may be published from the owner's computer (same Dockerfile):

```sh
docker build -f website/Dockerfile -t ghcr.io/ferhatte10/tunnelbird-website:latest .
docker push ghcr.io/ferhatte10/tunnelbird-website:latest
```

Then merge `feat/tunnelbird-site` in `ferhat-k8s`: Argo CD creates the app,
Image Updater pins the digest, and the site answers within a few minutes.

## Steps

1. Owner validates this plan and the decisions above.
2. Design directions → owner picks → `website/DESIGN.md`.
3. Skeleton: SvelteKit, i18n, theme, layout, 404, contrast tests.
4. Pages Home, Install, Docs, Privacy, Changelog, Download; screenshots.
5. `install.sh` with its tests.
6. Review by an independent read-only agent (design + technical), fixes.
7. Image + workflow; Kubernetes manifests; DNS; first deployment.
8. Signing key (owner), CronJob, first repository publication after `v0.1.0`.
9. End-to-end check: fresh Ubuntu VM → `curl … | sh` → app installed → bump to
   a test version → it shows up in the system updates and in the app.
