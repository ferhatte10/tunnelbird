# Tunnelbird website: design

Direction validated by the owner (2026-10-30): **A+B**, the night atmosphere
of the app icon with the clarity of GNOME-style pages, in two themes. The
mock-ups are on the design canvas “Tunnelbird site directions” (pages
“A+B · Nuit” and “A+B · Jour”). Read this before changing a page.

## Identity

- The app icon as is (`static/icon.svg`, copied from the app): never redrawn.
- One motif: the swallow of the icon, at the end of a thin flight line in the
  hero (`FlightLine.svelte`), and alone on the error page. Nowhere else.
- The red and blue triangle of the logo, small, before section titles
  (`TriangleMark.svelte`).
- Blue (`--accent`, `--link`, `--icon`) is for what can be clicked; red
  (`--mark-red`) only marks: current page underline, the triangle.
- Real screenshots of the app (`make screenshots` in the repository), in the
  visitor's language, the dark ones with the dark theme. Never mock-ups drawn
  in HTML.

## Themes and colours

Tokens in `src/app.css`, `:root` (day) and `:root[data-theme='dark']`
(night); components use them through Tailwind (`bg-bg`, `text-text-2`…),
never raw colours. The theme follows the system, the sun/moon button
overrides it (kept in localStorage), applied by `static/theme.js` before the
first paint. `npm run check` fails if a text/background pair drops under
4.5:1 (3:1 for icons) in either theme (`scripts/contrast.js`).

## Type

- Headings: Bricolage Grotesque (variable), extra bold, tight.
- Text: Atkinson Hyperlegible, 18 px base: made for legibility, fits a
  non-technical audience.
- Monospace only for commands.
- Both self-hosted (fontsource): no request to Google.

## Rules

- No AI-template habits: no pulsing status dot, no spaced uppercase mono
  overline, no row of key figures, no blurred halo or background grid, no
  bordered cards everywhere.
- Pointer cursor on everything clickable (set globally in `app.css`).
- Every text in `messages/en.json` and `messages/fr.json`, nothing hard-coded
  (command lines excepted).
- Phone first: no horizontal scroll at 375 px, touch targets of 44 px.
- No legal or trademark notice; WireGuard® only as a tunnel type.
- Windows and macOS are “coming soon”, honestly: no fake download.
- Honest claims only: no “trusted by” with the author's own projects, no
  invented numbers.
