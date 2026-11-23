{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.parts.url = "github:hercules-ci/flake-parts";
  inputs.my-nix = {
    url = "github:nmrshll/my-nix";
    inputs.nixpkgs.follows = "nixpkgs";
    inputs.fp.follows = "parts";
  };

  outputs = inputs@{ parts, my-nix, ... }: parts.lib.mkFlake { inherit inputs; } {
    systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
    imports = builtins.attrValues my-nix.flakeModuleGroups.essentials ++ [
      my-nix.flakeModules.rust
    ];

    aiSkills.enable = false;

    perSystem = { lib, pkgs, ... }:
      let
        repoRoot = ''$(git rev-parse --show-toplevel 2>/dev/null || pwd)'';
        strategies = [
          "VolatilityStrat1"
          "VolatilityStrat2"
          "BoostDecay"
          "Variance"
        ];

        scripts = {
          fbuild = "forge build --contracts contracts";
          ftest = "forge test -vvv --contracts contracts";
          otest = "cargo test -p optimizer -- --nocapture";
          simbuild = ''
            cd "${repoRoot}/amm_sim_rs" && maturin develop --release
          '';
          simtest = ''
            set -e
            simbuild
            ${lib.concatMapStringsSep "\n" (name: ''amm-match run "${repoRoot}/contracts/src/${name}.sol" --simulations 10'') strategies}
          '';
          snap = "forge snapshot";
          cov = "forge coverage";
          clean = "forge clean";
          fcheck = ''
            set -e
            clean
            fbuild
            ftest
            snap
          '';
          optim = ''cargo run -p optimizer -- "$@"'';
          compet = ''
            export UV_PROJECT_ENVIRONMENT="$VIRTUAL_ENV"
            uv run python -c "from amm_competition.cli import main; main()" "$@"
          '';
          best = "cargo test -p optimizer -- list_top_20 --no-capture --ignored";
          deploy-local = ''
            set -e
            if ! command -v anvil &> /dev/null; then
              echo "Anvil is unavailable" >&2
              exit 1
            fi
            if ! nc -z localhost 8545 2>/dev/null; then
              echo "Start Anvil before deploying" >&2
              exit 1
            fi
            forge script script/Deploy.s.sol --rpc-url http://localhost:8545 --broadcast
          '';
          uv-venv = ''
            set -e
            if [ -z "$VIRTUAL_ENV" ]; then
              echo "Enter the dev shell to set VIRTUAL_ENV" >&2
              exit 1
            fi
            if [ ! -d "$VIRTUAL_ENV" ]; then
              uv venv --python 3.13 "$VIRTUAL_ENV"
            fi
            uv pip install --python "$VIRTUAL_ENV/bin/python" -e "${repoRoot}"
          '';
        };
      in
      {
        rust.toolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" ];
        };
        rust.kache.enable = false;

        myDevShell = {
          toolchain = "nixpkgs";
          buildInputs = with pkgs; [ foundry git maturin python313 uv ];
          inherit scripts;
          env = {
            FOUNDRY_PROFILE = "default";
            ETH_RPC_URL = "http://localhost:8545";
            RUST_BACKTRACE = "1";
          };
          shellHooks = {
            configure-editors = lib.mkForce "";
            configure-zed = lib.mkForce "";
            amm = ''
              export VIRTUAL_ENV="${repoRoot}/.venv"
              if uv-venv; then
                source "$VIRTUAL_ENV/bin/activate"
              else
                echo "Could not set up the project Python environment" >&2
              fi
              echo "Foundry development environment ready"
              echo "Available commands: ${lib.concatStringsSep ", " (builtins.attrNames scripts)}"
              echo "To start a local testnet: anvil"
              echo "To deploy to it: deploy-local (after anvil is running)"
            '';
          };
        };
      };
  };
}
