# AUR package

`tonal/` holds the PKGBUILD for the [AUR](https://aur.archlinux.org/) package
`tonal`. It builds from the GitHub release tarball of tag `v$pkgver`.

`sha256sums=()` is left empty on purpose. It is filled in at release time,
after the tag exists. Never publish with `'SKIP'`.

## Publishing a release

1. Add an SSH public key to your AUR account (My Account > SSH Public Key),
   and point `~/.ssh/config` at it:

       Host aur.archlinux.org
         IdentityFile ~/.ssh/aur
         User aur

2. Bump `version` in `Cargo.toml`, commit, then tag and push `v<version>`.
   In `PKGBUILD` set `pkgver` to the same version and reset `pkgrel=1`.

3. Fill in the checksum and regenerate `.SRCINFO`:

       cd packaging/aur/tonal
       updpkgsums
       makepkg --printsrcinfo > .SRCINFO

4. Test build in a clean copy outside the repo (`makepkg -f`, optionally
   `namcap` on the PKGBUILD and the resulting package), then commit the
   updated `PKGBUILD` and `.SRCINFO` here.

5. Push to the AUR:

       git clone ssh://aur@aur.archlinux.org/tonal.git aur-tonal
       cp packaging/aur/tonal/PKGBUILD packaging/aur/tonal/.SRCINFO aur-tonal/
       cd aur-tonal
       git add PKGBUILD .SRCINFO
       git commit -m "Update to <version>"
       git push

   The first push to an unused name creates the package.

## Local test while the repo is private

    git archive --prefix=tonal-0.1.0/ HEAD -o /tmp/tonal-0.1.0.tar.gz

Copy the PKGBUILD to a scratch directory, point `source=` at
`file:///tmp/tonal-$pkgver.tar.gz`, run `updpkgsums`, then `makepkg -f`.
