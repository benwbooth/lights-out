{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        lights-out-bin = pkgs.rustPlatform.buildRustPackage {
          pname = "lights-out";
          version = "0.1.0";
          src = ./lights-out;
          cargoLock.lockFile = ./lights-out/Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.hidapi pkgs.udev ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
            pkg-config
            hidapi
            udev
          ];

          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
        };

        packages.lights-out-bin = lights-out-bin;

        packages.default = pkgs.stdenv.mkDerivation {
          pname = "lights-out";
          version = "0.1.0";
          src = ./.;

          nativeBuildInputs = [ pkgs.makeWrapper ];

          installPhase = ''
            mkdir -p $out/bin

            # Install the Rust binary
            cp ${lights-out-bin}/bin/lights-out $out/bin/lights-out

            # Install the wrapper script with proper paths
            substitute lights-out.sh $out/bin/lights-out.sh \
              --replace 'LEDCTL="$SCRIPT_DIR/lights-out/target/release/lights-out"' \
                        'LEDCTL="${lights-out-bin}/bin/lights-out"'

            chmod +x $out/bin/lights-out.sh

            # Wrap the script to include openrgb in PATH
            wrapProgram $out/bin/lights-out.sh \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.openrgb ]}
          '';
        };
      });
}
