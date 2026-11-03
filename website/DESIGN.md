# Tunnelbird website: design

Direction validated by the maintainer (2026-10-30): **A+B**, the night atmosphere
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

## Motion

Motion says something: what to read first, that a section starts, where the
new theme comes from. Never a loop that keeps moving, never a decoration
alone. Tokens in `src/app.css` (`--ease-out`, `--motion-fast` 160 ms,
`--motion-base` 320 ms, `--motion-slow` 700 ms, `--motion-draw` 1.4 s); only
`transform`, `opacity` and SVG strokes are animated.

- **Hero arrival** (`.enter`, `--d` = order): title, lead, command, buttons
  rise 1rem in a 90 ms cascade; the screenshot settles from slightly lower.
- **Signature moment**: the flight line draws itself, then the swallow lands
  at its end (`FlightLine.svelte`). Once, on load.
- **Sections on scroll** (`use:reveal`, `--i` = order in a group): fade and
  rise once when they enter the screen; the two halves of the section
  triangle join. What is already on screen at load is never hidden.
- **Between pages** (View Transitions): fade-through, the old page leaves in
  140 ms, then the new one rises in; the header stays still. Changing
  language (a full load) does the same.
- **Day and night**: the new theme spreads as a circle from the button.
- **Feedback**: buttons press (`.press`, scale 0.97), a new install tab's
  content fades in.
- **The app's film** (`DemoVideo.svelte`): only its poster is part of the
  page (lazy); the muted video is created in the browser, `preload="none"`,
  plays on hover or focus with a mouse, when 60 % visible on touch screens,
  in the visitor's language and theme, in portrait (4:5) on phones where 16:9
  would be unreadable, never with reduced motion or data
  saving; a button pauses or plays it at any time.
- **Reduced motion** (`prefers-reduced-motion`): everything is shown at once,
  no delay. **Without JavaScript**: nothing is ever hidden.

## Rules

- No AI-template habits: no pulsing status dot, no spaced uppercase mono
  overline, no row of key figures, no blurred halo or background grid, no
  bordered cards everywhere.
- Pointer cursor on everything clickable (set globally in `app.css`).
- Every text in `messages/en.json` and `messages/fr.json`, nothing hard-coded
  (command lines excepted).
- Phone first: no horizontal scroll at 375 px, touch targets of 44 px.
- Right from the first frame: what depends on the theme (icons, labels,
  screenshots) follows `<html data-theme>` through CSS, never a state set
  once the page starts; the fonts are preloaded; the hero images load at
  once in both themes. A reload must not flash or shift.
- Languages: each under its own name (`Intl.DisplayNames`, no labels to
  write); phones show up to 3 side by side in the menu, more as a list that
  scrolls; the desktop menu scrolls past its height. Changing language loads
  the page (see `src/lib/language.ts`). First visit of `/`: a browser whose
  first language is French opens `/fr/`, any other stays in English
  (`static/language.js`, before the first paint); a language chosen in the
  menu wins, kept in the browser like the theme (no cookie).
- French text: no-break space before `: ; ? !` and inside `« »`.
- No legal or trademark notice; WireGuard® only as a tunnel type.
- Windows and macOS are “coming soon”, honestly: no fake download.
- Honest claims only: no “trusted by” with the author's own projects, no
  invented numbers.
