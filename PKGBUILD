# Maintainer: dominic84p
pkgname=vide-de-git
pkgver=0.1.0
pkgrel=1
pkgdesc="A Wayland compositor and Windows 10-style desktop environment written in Rust"
arch=('x86_64')
url="https://github.com/dominic84p/vide-de"
license=('MIT')
depends=(
    'wayland'
    'libxkbcommon'
    'mesa'
    'libinput'
    'seatd'
    'libdrm'
    'xorg-xwayland'
    'udisks2'
    'networkmanager'
    'pipewire'
    'wireplumber'
)
makedepends=('cargo' 'git')
optdepends=(
    'kwallet: Secret Service keyring support'
    'bluez-utils: Bluetooth tray control'
    'konsole: fallback terminal'
)
provides=('vide-de')
conflicts=('vide-de')
source=()

build() {
    cargo build --release --workspace --features vibe-comp/tty
}

package() {
    make install DESTDIR="$pkgdir" PREFIX="/usr"
}
