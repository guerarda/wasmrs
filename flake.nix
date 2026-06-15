{
  description = "wasmrs — WebAssembly runtime in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "rustfmt" "clippy" ];
        };
      in {
        devShells.default = pkgs.mkShell {
          packages = [
            rust
            pkgs.wabt
          ];

          shellHook = ''
            if [ -x /Library/Developer/CommandLineTools/usr/bin/lldb ]; then
              export PATH=/Library/Developer/CommandLineTools/usr/bin:$PATH
            fi
          '';
        };
      });
}
