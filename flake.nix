{
  description = "codexlens — a lens onto your Codex usage";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    nix-github-release = {
      url = "github:lambdalisue/nix-github-release/v0.1.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane, nix-github-release }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Single source of the Rust version, shared by the dev shell and the
        # package build (rust-toolchain.toml).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          # rusqlite is built with the `bundled` feature, so SQLite compiles from
          # C against the stdenv C toolchain — no system sqlite/pkg-config needed.
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # The tag this flake was fetched at, from VERSION; see
        # github:lambdalisue/nix-github-release. Only the package is stamped
        # with it, so the dependency layer above stays cached across releases.
        version = nix-github-release.lib.version { inherit self; };
        codexlens = craneLib.buildPackage (nix-github-release.lib.stampCargo { inherit pkgs version; }
          (commonArgs // { inherit cargoArtifacts; }));
      in
      {
        # `nix build` / `nix run github:selphie-til/codexlens -- summary`; pin a
        # release with `github:selphie-til/codexlens/vX.Y.Z`.
        packages.default = codexlens;
        apps.default = flake-utils.lib.mkApp { drv = codexlens; };

        # `nix develop` — the pinned tools, same ones CI uses via `just`.
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            just
            git
            jq
            sqlite # the `sqlite3` CLI, handy for poking at the store
            nixpkgs-fmt
          ];
          env.RUST_BACKTRACE = "1";
          shellHook = ''
            echo "codexlens dev shell — $(rustc --version)"
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
