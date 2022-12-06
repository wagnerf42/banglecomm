{pkgs ? import <nixpkgs> {}}:
let
  fenix = import (fetchTarball "https://github.com/nix-community/fenix/archive/main.tar.gz") { };
in
pkgs.mkShell {
  buildInputs=with pkgs; [
    pkgconfig
    dbus

    (
      with fenix;
      combine (
        with stable; [
          cargo
          clippy-preview
          stable.rust-src
          rust-analyzer
          rust-std
          rustc
          rustfmt-preview
        ]
      )
    )
  ];
}
