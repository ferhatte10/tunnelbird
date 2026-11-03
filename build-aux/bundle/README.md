# Self-contained package

For distributions that are not Ubuntu, Debian or Fedora (those get the
repositories): a tarball with Tunnelbird, GTK 4 and libadwaita inside, which
`install.sh` unpacks in the user's home folder. No store, no administrator.

```sh
build-aux/bundle/build.sh                      # target/bundle/tunnelbird-VERSION-linux-ARCH.tar.gz
build-aux/bundle/smoke.sh target/bundle/*.tar.gz
```

- `build.sh` downloads `linuxdeploy` and its GTK plugin once, and checks them
  against the digests written at the top of the script (never run unchecked).
  Update them there, from the release page, when moving to a newer version.
- The plugin's start-up file forces an old GTK theme, which switches libadwaita
  off, and X11: the script replaces its content with its own (same file name,
  `AppRun` sources it by name)
  and removes the schema overrides of the build machine's desktop.
- The modules GTK loads on demand (SVG, image formats) get their own library search path
  (`patchelf`), so no `LD_LIBRARY_PATH` leaks into the programs the app starts.
- It is built on Ubuntu 24.04 (glibc 2.39): a package built on a newer system
  does not start on older ones. Do not publish a package built on your own PC.
- What it does not carry: the C library, NetworkManager, the graphics drivers,
  fonts (also the emoji one that draws the flags), and what every desktop
  already has: fontconfig, HarfBuzz, FriBiDi, `libgpg-error`, `libcom_err`, the
  Wayland and X11 client libraries, and `shared-mime-info` (without its type
  database GTK cannot tell an SVG icon from any file). `install.sh` checks them.
- The pixbuf loaders' list needs the real install path: it is written at the
  first start, in `~/.cache/tunnelbird`, from `loaders.cache.in`.
- How it was checked: built in an Ubuntu 24.04 container, then started in demo
  mode (16 screenshots, no icon error) on Ubuntu 24.04, Debian 13, Fedora 42 and
  Arch Linux containers with a minimal desktop's libraries; never on a real
  desktop of those distributions, nor on arm64.
