# Contributing to Tunnelbird

Thanks for helping! Start with [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md):
it explains each module in a few lines.

## Development setup

Install the build dependencies listed in the [README](README.md), then:

```sh
make dev-install                        # once: dock icon and menu entry for the dev build (~/.local, no sudo)
cargo run                               # run the app
G_MESSAGES_DEBUG=tunnelbird cargo run     # with debug logs
make check                              # format, lints, tests, desktop file
```

## Code rules

- **Never panic on runtime data.** No `unwrap()`/`expect()` on anything coming
  from files, D-Bus or the system. Return a `Result`, then either recover or
  report it with `Ui::report_error` (log + toast).
- **Every user-visible string goes through `gettext()`**, in English, with
  named placeholders (`{name}`), never `format!` positional arguments.
- **Follow GNOME conventions**: [GNOME Human Interface Guidelines](https://developer.gnome.org/hig/),
  title case for titles and buttons, `…` for actions that open a dialog.
- **Comments explain why**, not what. Module headers (`//!`) say what the
  module is for.
- Run `make check` (format, lints and tests of both crates, desktop and
  store files) before opening a pull request.

## Commits and pull requests

- One topic per pull request, with a short description of the user-visible change.
- Commit messages: `feat: …`, `fix: …`, `docs: …`, `i18n: …`, `chore: …`.

## Translations

See [docs/TRANSLATING.md](docs/TRANSLATING.md).
