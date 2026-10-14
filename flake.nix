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

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rust = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "rustfmt"
            "clippy"
            "llvm-tools-preview"
          ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            rust
            pkgs.cargo-fuzz
            pkgs.wabt
          ];

          shellHook = ''
            if [ -x /Library/Developer/CommandLineTools/usr/bin/lldb ]; then
              export PATH="$PATH:/Library/Developer/CommandLineTools/usr/bin"
            fi
            export LIBRARY_PATH="${pkgs.libiconv}/lib''${LIBRARY_PATH:+:$LIBRARY_PATH}"
          '';
        };
      }
    );
}
