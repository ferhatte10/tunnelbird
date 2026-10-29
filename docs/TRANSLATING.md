# Translating Tunnelbird

Tunnelbird uses [gettext](https://www.gnu.org/software/gettext/). English is the
source language; each other language is one file in `po/`.

## Add a language

1. Refresh the template after code changes:

   ```sh
   xgettext --from-code=UTF-8 --language=Rust --keyword=gettext \
     --files-from=po/POTFILES --output=po/tunnelbird.pot
   ```

2. Create your file (here German):

   ```sh
   msginit --input=po/tunnelbird.pot --locale=de --output-file=po/de.po
   ```

3. Translate every `msgstr` in `po/de.po`.
4. Add `de` on its own line in `po/LINGUAS`, and `("de", "Deutsch")` (the
   language's own name) to `LANGUAGES` in `src/settings.rs` so it appears in Preferences.
5. Test: `LANGUAGE=de cargo run`

## Update an existing language

```sh
msgmerge --update po/fr.po po/tunnelbird.pot
```

Then translate the new or `fuzzy` entries.

## Rules

- Keep placeholders such as `{name}`, `{file}`, `{rx}` exactly as they are;
  you may move them inside the sentence.
- `_` before a letter marks the keyboard shortcut (`_Delete` → Alt+D).
- For `translator-credits`, put your name (one per line for several people).
- Check your file: `msgfmt --check -o /dev/null po/de.po`
