# Packaging for Linux

On Windows, Setup carries the app and installs it. On Linux a package or the AppImage installs the
app, and setup travels beside it without the app inside: it puts the runtime, Node, uv, Python and
the models in the person's own folder, `~/.local/opt/demido-studio`, where the app finds them (see
[Architecture](architecture.md)). So every Linux release is the same few files, laid out the same
way.

## What a Linux release is

`pnpm build` on Linux (`scripts/build-release.mjs`) builds the app and setup and lays them out as a
package installs them:

```
usr/bin/demido-studio -> ../lib/demido-studio/demido-studio
usr/lib/demido-studio/demido-studio        the app
usr/lib/demido-studio/demido-setup         setup, runtimes only
usr/lib/demido-studio/resources/           the market service and the default skills
usr/share/applications/demido-studio.desktop
usr/share/icons/hicolor/{32x32,64x64,128x128,256x256,512x512,scalable}/apps/demido-studio.*
usr/share/metainfo/app.demido.studio.metainfo.xml
usr/share/licenses/demido-studio/LICENSE
usr/share/doc/demido-studio/THIRD_PARTY_NOTICES.md
```

The app finds its resources and setup beside its real path, so the tree runs from anywhere: from
`/usr` once a package installs it, or unpacked in a home folder.

- **The tarball**, `demido-studio-<version>-linux-x86_64.tar.gz`, is that tree in a folder named
  `demido-studio-<version>-x86_64`. Both AUR packages install it as it is.
- **The AppImage**, `Demido-Studio-<version>-x86_64.AppImage`, is made by Tauri's bundler from the
  same binaries. It carries WebKitGTK and GStreamer, keeps setup in `usr/bin` beside the app and
  the resources in `usr/lib/Demido Studio`. It is the file the Linux updater downloads, so it is
  the one that gets signed. An AppImage's files exist only while it runs, so the app starts the
  setup inside it as `$APPIMAGE --setup …`, and `main.rs` hands over to it.

The desktop entry is named `demido-studio.desktop` because that is the app's Wayland app id (the
executable's name): the window then groups with its launcher, and the desktop's portals (the one
that asks about the camera, say) name the app from it.

Releases are built on Ubuntu 22.04, the oldest system with WebKitGTK 4.1 they are for, so the
binaries need no newer glibc than it has. They link WebKitGTK 4.1, GTK 3, libsoup 3, GLib, cairo,
gdk-pixbuf and D-Bus; WebKitGTK plays and records sound through GStreamer's base and good plugins.

## What the runtimes need from the system

llama.cpp's Linux builds load some of what they need from the system, which the Windows builds
carry themselves:

| Runtime | From the system | On Arch |
|---|---|---|
| CUDA | The NVIDIA driver (`libcuda`); the CUDA runtime libraries come with llama.cpp | `nvidia-utils` |
| ROCm | ROCm's HIP runtime and hipBLAS, nothing of ROCm comes with llama.cpp | `rocm-hip-runtime`, `hipblas` |
| Vulkan | The Vulkan loader and a driver for the card | `vulkan-icd-loader`, and `vulkan-radeon`, `vulkan-intel` or `nvidia-utils` |

Setup offers ROCm and Vulkan only when it finds their libraries where the loader looks
(`demido_hardware::SystemLibraries`), and says what to install otherwise. The packages list them
as optional dependencies.

## The AUR packages

`packaging/aur` holds two packages:

- **`demido-studio-bin`** installs a release's tarball. It follows stable releases.
- **`demido-studio-git`** builds the latest commit of `main` from source
  (`node scripts/build-release.mjs --tree`). Its version comes from `git describe`, so it needs no
  change between releases.

Both share `demido-studio.install`, which tells the person where the runtimes, models and data go,
since removing the package leaves them in their home folder.

To try either from a checkout:

```bash
cd packaging/aur/demido-studio-git
# build this checkout rather than GitHub's main: source=("$_pkgname::git+file:///path/to/demido-studio#branch=<branch>")
makepkg -si
```

### Publishing to the AUR

The first time, once: create an account on [aur.archlinux.org](https://aur.archlinux.org), add your
SSH public key to it, and create each package by pushing it:

```bash
git clone ssh://aur@aur.archlinux.org/demido-studio-bin.git
cp packaging/aur/demido-studio-bin/{PKGBUILD,.SRCINFO,demido-studio.install} demido-studio-bin/
cd demido-studio-bin && git add -A && git commit -m "Initial import: demido-studio-bin <version>" && git push
```

`demido-studio-git` goes the same way, with `makepkg --printsrcinfo > .SRCINFO` run in its folder
first.

For every stable release after that, the release workflow points `demido-studio-bin` at it
(`scripts/aur.mjs`: the version, `pkgrel=1` and the tarball's SHA-256, in the `PKGBUILD` and
`.SRCINFO`) and keeps both as the artifact `aur-demido-studio-bin-<version>` of its run. Download
it, copy its files into your clone of the AUR repository, commit ("Update to <version>") and push.
Commit the same files to this repository too, so `packaging/aur` stays at the latest release. The
`-git` package needs nothing: it builds whatever `main` is.

To set the version by hand instead:

```bash
node scripts/aur.mjs 0.6.0 "$(sha256sum demido-studio-0.6.0-linux-x86_64.tar.gz | cut -d' ' -f1)"
```

Pre-releases go to GitHub only: pacman would read the `-` of `0.6.0-beta.1` as the start of
`pkgrel`, and `scripts/aur.mjs` refuses them.

### Changing the packages

When the app links a new library or the runtimes need another part of the system, change
`depends` or `optdepends` in both PKGBUILDs alike; `readelf -d` on the binaries shows what they
link. Then run `makepkg --printsrcinfo > .SRCINFO` in each folder (`pnpm test:scripts` checks that
`demido-studio-bin`'s `.SRCINFO` matches its `PKGBUILD` where `makepkg` is installed), and bump
`pkgrel` of `demido-studio-bin` when the release stays the same.
