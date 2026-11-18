{
  description = "kdl-kit: KDL v2 for Rust (parser, serde, formatter)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            }
          )
        );
      rustToolchain = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      rustPlatform =
        pkgs:
        pkgs.makeRustPlatform {
          cargo = rustToolchain pkgs;
          rustc = rustToolchain pkgs;
        };
    in
    {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (rustToolchain pkgs)
            pkgs.cargo-insta
            pkgs.cargo-nextest
            pkgs.cargo-deny
          ];
          RUST_SRC_PATH = "${rustToolchain pkgs}/lib/rustlib/src/rust/library";
        };
      });

      formatter = forAll (pkgs: pkgs.nixfmt);
    };
}
