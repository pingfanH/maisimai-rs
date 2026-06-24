{
  description = "maisimai-rs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        commonEnv = ''
          export CARGO="${pkgs.cargo}/bin/cargo"
          export RUSTC="${pkgs.rustc}/bin/rustc"
          export CARGO_TARGET_DIR="$PWD/target/nix"
        '';
        buildScript = pkgs.writeShellApplication {
          name = "maisimai-build";
          runtimeInputs = with pkgs; [ cargo rustc ];
          text = ''
            ${commonEnv}
            if [ "$#" -eq 0 ]; then
              set -- --lib
            fi
            exec "$CARGO" build "$@"
          '';
        };
      in {
        packages.default = buildScript;

        apps.default = {
          type = "app";
          program = "${buildScript}/bin/maisimai-build";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
          ];

          shellHook = commonEnv;
        };
      });
}
