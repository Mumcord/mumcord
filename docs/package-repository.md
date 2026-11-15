# Package repository (Linux and FreeBSD)

Mumcord has its own signed package repository at **<https://packages.mumcord.app>**. Once it is added, your system's normal update tool (`apt upgrade`, `dnf upgrade`, `zypper update`, `pacman -Syu`, `pkg upgrade`) keeps Mumcord up to date, and the in-app updater shows that command instead of updating by itself.

The easiest way is the install script, which asks how you want to install Mumcord and sets up the repository for you:

```sh
curl -fsSL https://github.com/Mumcord/mumcord/releases/latest/download/install.sh | sh
```

(On FreeBSD use `fetch -qo -` instead of `curl -fsSL`.) The steps below do the same by hand.

The repository carries stable releases only, and only the newest one. Release candidates are not published there.

## Signing key

APT, RPM and pacman use one OpenPGP key:

- **Name:** Mumcord Package Signing `<packages@mumcord.app>`
- **Fingerprint:** `A409 6249 A499 587C 9AEF  13E7 F022 C81D 47D2 BBEB`
- **Download:** <https://packages.mumcord.app/mumcord.asc> (also in this repository: [`packaging/repo/mumcord.asc`](../packaging/repo/mumcord.asc))
- **Valid until:** 2031-11-15

FreeBSD's `pkg` uses its own RSA key: <https://packages.mumcord.app/mumcord.pub> (also [`packaging/repo/mumcord.pub`](../packaging/repo/mumcord.pub)), SHA-256 of the file `9c03cc83f882d84192c8489d070c519ac2ec190c7eb61a53161913d330aeafa6`.

Compare the fingerprint before you trust the key, e.g. with `gpg --show-keys mumcord.asc`. How the keys are kept and replaced is described in [CODE_SIGNING.md](../CODE_SIGNING.md#linux-and-freebsd-package-repository).

## Debian, Ubuntu, Linux Mint (APT)

```sh
sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSLo /etc/apt/keyrings/mumcord.asc https://packages.mumcord.app/mumcord.asc
sudo curl -fsSLo /etc/apt/sources.list.d/mumcord.sources https://packages.mumcord.app/mumcord.sources
sudo apt update
sudo apt install mumcord
```

The key is used only for this repository (`Signed-By:` in `mumcord.sources`), not for your whole system; `apt-key` is not needed. Works on amd64 and arm64.

## Fedora (DNF)

```sh
sudo rpm --import https://packages.mumcord.app/mumcord.asc
sudo curl -fsSLo /etc/yum.repos.d/mumcord.repo https://packages.mumcord.app/mumcord.repo
sudo dnf install mumcord
```

Both the packages and the repository metadata are signed (`gpgcheck=1`, `repo_gpgcheck=1`).

## openSUSE (zypper)

```sh
sudo rpm --import https://packages.mumcord.app/mumcord.asc
sudo zypper addrepo https://packages.mumcord.app/mumcord.repo
sudo zypper refresh
sudo zypper install mumcord
```

## Arch Linux (pacman)

```sh
curl -fsSLO https://packages.mumcord.app/mumcord.asc
sudo pacman-key --add mumcord.asc
sudo pacman-key --lsign-key A4096249A499587C9AEF13E7F022C81D47D2BBEB
```

Then add this to the end of `/etc/pacman.conf`:

```ini
[mumcord]
Server = https://packages.mumcord.app/arch/$arch
```

and install with `sudo pacman -Syu mumcord-bin`. The package has the same name as the AUR package `mumcord-bin`, so an AUR install switches to the repository by itself on the next `pacman -Syu`.

## FreeBSD 15.x (pkg)

As root:

```sh
mkdir -p /usr/local/etc/pkg/keys /usr/local/etc/pkg/repos
fetch -o /usr/local/etc/pkg/keys/mumcord.pub https://packages.mumcord.app/mumcord.pub
fetch -o /usr/local/etc/pkg/repos/mumcord.conf https://packages.mumcord.app/mumcord.conf
pkg update
pkg install mumcord
```

Its dependencies come from the official FreeBSD repository as usual.

## Removing the repository

- **APT:** `sudo rm /etc/apt/sources.list.d/mumcord.sources /etc/apt/keyrings/mumcord.asc`
- **DNF:** `sudo rm /etc/yum.repos.d/mumcord.repo`
- **zypper:** `sudo zypper removerepo mumcord`
- **pacman:** remove the `[mumcord]` block from `/etc/pacman.conf`, then `sudo pacman-key --delete A4096249A499587C9AEF13E7F022C81D47D2BBEB`
- **FreeBSD:** `rm /usr/local/etc/pkg/repos/mumcord.conf /usr/local/etc/pkg/keys/mumcord.pub`

`install.sh --uninstall` does this for you as well.
