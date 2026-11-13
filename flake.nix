{
  description = "Rust libraries and apps for OpenIPC FPV ground stations";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane/v0.20.3";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      crane,
      rust-overlay,
      ...
    }@inputs:
    let
      perSystem = flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        toolchain = pkgs.rust-bin.stable.latest.minimal.override {
          extensions = [ "clippy" "rustfmt" "rust-src" "rust-analyzer" ];
          targets = [ "wasm32-unknown-unknown" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

        runtimeLibs = with pkgs; [
          alsa-lib
          libclang.lib
          libdrm
          libglvnd
          libva
          libx11
          libxcb
          libxcursor
          libxi
          libxkbcommon
          libxrandr
          mesa
          wayland
        ];

        src = craneLib.cleanCargoSource (craneLib.path ./.);

        commonArgs = {
          inherit src;
          strictDeps = true;
          pname = "openipc-rs";
          version = "0.1.40";
          nativeBuildInputs = with pkgs; [
            clang
            pkg-config
          ];
          buildInputs = runtimeLibs;
          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          env = {
            CARGO_BUILD_JOBS = "2";
          };
        };

        cargoArtifacts = craneLib.buildDepsOnly (
          commonArgs
          // {
            cargoExtraArgs = "--workspace";
          }
        );

        mkBin =
          {
            pname,
            cargoExtraArgs,
            extraBuildInputs ? [ ],
            extraNativeBuildInputs ? [ ],
            postInstall ? "",
          }:
          craneLib.cargoBuild (
            commonArgs
            // {
              inherit cargoArtifacts pname postInstall;
              cargoExtraArgs = cargoExtraArgs;
              buildInputs = runtimeLibs ++ extraBuildInputs;
              nativeBuildInputs = [
                pkgs.clang
                pkgs.pkg-config
              ] ++ extraNativeBuildInputs;
            }
          );

        nebulus = mkBin {
          pname = "nebulus";
          cargoExtraArgs = "-p nebulus --bin nebulus";
          extraNativeBuildInputs = [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram $out/bin/nebulus --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}"
          '';
        };
      in
      {
        packages = rec {
          default = nebulus;
          inherit nebulus;
          cli = mkBin {
            pname = "openipc-cli";
            cargoExtraArgs = "-p openipc-cli";
          };
          wfb = mkBin {
            pname = "wfb-rs";
            cargoExtraArgs = "-p wfb-rs";
          };
          all = pkgs.buildEnv {
            name = "openipc-rs-all";
            paths = [
              nebulus
              cli
              wfb
            ];
          };
        };

        checks = {
          fmt = craneLib.cargoFmt {
            inherit src;
            cargoFmtExtraArgs = "--all --check";
          };
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--workspace --all-targets --no-deps --locked -- -D warnings";
            }
          );
          test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoTestExtraArgs = "--workspace --locked";
            }
          );
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            toolchain
            binaryen
            bun
            cargo-release
            git-cliff
            trunk
          ];
          nativeBuildInputs = with pkgs; [
            clang
            pkg-config
          ];
          buildInputs = runtimeLibs;
          shellHook = ''
            export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}"
            export RUST_SRC_PATH="${toolchain}/lib/rustlib/src/rust/library"
          '';
        };

        formatter = pkgs.nixfmt-rfc-style;
        }
      );
    in
    perSystem
    // {
      nixConfig = {
        max-jobs = 2;
      };
    };
}