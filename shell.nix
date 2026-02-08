{ pkgs ? import (fetchTarball
  "https://github.com/NixOS/nixpkgs/archive/nixos-unstable.tar.gz") { } }:

with pkgs;

mkShell { nativeBuildInputs = [ cargo rustc rust-analyzer rustfmt clippy ]; }
