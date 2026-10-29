# Flathub submission

Everything Flathub needs, ready to copy into the submission pull request.
`build-aux/flatpak/` stays the local build (sources from this folder); this
manifest builds the tagged release from GitHub, as Flathub requires.

## What is ready

- `fr.ferhatsn.Tunnelbird.json`: same build as the local one, source = the git
  tag. `x-checker-data` lets Flathub's bot open a pull request by itself for
  every new `vX.Y.Z` tag.
- Store listing (`data/fr.ferhatsn.Tunnelbird.metainfo.xml`), checked against
  Flathub's quality guidelines:
  - summary of 32 characters, no app name, no “app/client”, no full stop
    (English and French);
  - six screenshots per language, GNOME's default look, window with its title
    bar, rounded corners and shadow, 640×700 window at 2x, one-sentence
    captions; made with `make screenshots` (demo mode, made-up addresses);
  - SVG icon, brand colours for light and dark, OARS rating, release notes,
    bug tracker, source and translation links.
- Permissions explained in `build-aux/flatpak/README.md` (reviewers ask why a
  VPN client talks to NetworkManager on the system bus: it is the whole app).

## Prerequisites (owner)

1. The GitHub repository is **public** (Flathub builds from it, the screenshots
   are served from it).
2. Tag `v0.1.0` pushed (see `docs/plans/ci-and-release.md`).
3. **Domain check**: the app ID `fr.ferhatsn.Tunnelbird` belongs to
   `ferhatsn.fr`. After the app is accepted, Flathub's developer portal gives a
   token to publish at `https://ferhatsn.fr/.well-known/org.flathub.VerifiedApps.txt`;
   the app then shows the “verified” badge. The portfolio serves that file
   (static route).
4. A GitHub account able to open the pull request (the owner's).

## Steps

1. Put the tag's commit in the manifest:
   `git rev-list -n1 v0.1.0` → replace `COMMIT-OF-THE-v0.1.0-TAG`.
2. Check `cargo-sources.json` matches the tag's `Cargo.lock` (regenerate it as
   described in `build-aux/flatpak/README.md` if a dependency changed), and
   copy it next to the manifest.
3. Build and lint exactly as Flathub will:

   ```sh
   flatpak install flathub org.flatpak.Builder
   flatpak run org.flatpak.Builder --force-clean --sandbox --user --install \
     --install-deps-from=flathub --ccache --mirror-screenshots-url=https://dl.flathub.org/media \
     --repo=repo builddir build-aux/flathub/fr.ferhatsn.Tunnelbird.json
   flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest build-aux/flathub/fr.ferhatsn.Tunnelbird.json
   flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
   flatpak run fr.ferhatsn.Tunnelbird
   ```

4. Fork `flathub/flathub`, branch from `new-pr`, add the manifest and
   `cargo-sources.json`, open the pull request **against `new-pr`** titled
   `Add fr.ferhatsn.Tunnelbird`, and fill in the checklist. Public action:
   only with the owner's go-ahead.
5. Answer the review (usually about permissions), then `bot, build` in the
   pull request runs a test build. Once merged, Flathub creates
   `flathub/fr.ferhatsn.Tunnelbird`; later releases are pull requests there,
   opened by the `x-checker-data` bot.

## After acceptance

- Add the “Download on Flathub” badge to the README and the website.
- Inside the Flatpak, update notices come from the Flatpak portal
  (`src/update.rs`): the app offers “Update” and then “Restart”, nothing to do
  per release.
