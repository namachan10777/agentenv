{
  description = "agentenv - switch Claude Code / Codex / OpenCode profiles per shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));
    in
    {
      packages = forAllSystems (pkgs: rec {
        agentenv = pkgs.rustPlatform.buildRustPackage {
          pname = "agentenv";
          version = "0.3.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
          meta = {
            description = "Switch Claude Code / Codex / OpenCode profiles per shell";
            mainProgram = "agentenv";
          };
        };
        default = agentenv;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
          env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });
    };
}
