{
  description = "Local-first PDF/Markdown knowledge engine";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        version = cargoToml.workspace.package.version;
        isDarwin = pkgs.stdenv.isDarwin;
        darwinDeps = pkgs.lib.optionals isDarwin (
          with pkgs.darwin.apple_sdk.frameworks; [ Security SystemConfiguration ]
        );
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "shiro";
          inherit version;
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "shiro-cli" ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = darwinDeps;
          meta = {
            description = cargoToml.workspace.package.description or "shiro CLI";
            license = with pkgs.lib.licenses; [ mit asl20 ];
            mainProgram = "shiro";
          };
        };

        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            pkgs.rustc
            pkgs.cargo
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.rustfmt
            pkgs.pkg-config
          ];
          buildInputs = darwinDeps;
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      }
    );
}
