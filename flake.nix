{

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.parts.url = "github:hercules-ci/flake-parts";
  inputs.my-nix = { url = "github:nmrshll/nix-utils"; inputs.nixpkgs.follows = "nixpkgs"; inputs.fp.follows = "parts"; };

  outputs = inputs@{ parts, ... }: with builtins; parts.lib.mkFlake { inherit inputs; } ({ lib, ... }: {
    systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
    imports = lib.flatten [
      (attrValues inputs.my-nix.flakeModuleGroups.essentials)
      inputs.my-nix.flakeModules.rust
    ];

    perSystem = { pkgs, ... }:
      let
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" ];
          targets = [ ];
        };

        buildDeps = with pkgs; [ foundry uv maturin rustToolchain ]; # rust added by imported flake-part
        devDeps = [ ];



        strategies = [
          # "${bash.wd}/contracts/src/StarterStrategy.sol"
          # "${bash.wd}/contracts/src/WidenAfterBigTrades.sol"
          # "${bash.wd}/contracts/src/InventoryAware.sol"
          "${bash.wd}/contracts/src/VolatilityStrat1.sol"
          "${bash.wd}/VolatilityStrat1.sol"
          "${bash.wd}/contracts/src/VolatilityStrat2.sol"
          "${bash.wd}/contracts/src/BoostDecay.sol"
          "${bash.wd}/contracts/src/Variance.sol"
        ];

        bash.wd = ''$(git rev-parse --show-toplevel 2>/dev/null || echo \".\")'';
        bin = mapAttrs (name: pkg: "${pkg}/bin/${name}") scripts;
        scripts = mapAttrs pkgs.writeShellScriptBin {
          fbuild = '' forge build --contracts contracts'';
          ftest = '' forge test -vvv --contracts contracts'';
          otest = "cargo test -p optimizer -- --nocapture";
          simbuild = '' cd amm_sim_rs ; maturin develop --release ; source "$VIRTUAL_ENV/bin/activate" '';
          simtest = ''
            simbuild ; source "$VIRTUAL_ENV/bin/activate"
            ${concatStringsSep "\n" (map (s: "amm-match run ${s} --simulations 10") strategies)}
          '';
          snap = '' forge snapshot '';
          cov = '' forge coverage '';
          clean = '' forge clean '';
          fcheck = '' set -e
              ${bin.clean}
              ${bin.fbuild}
              ${bin.ftest}
              ${bin.snap}
            '';
          optim = '' cargo run -p optimizer -- '';
          compet = ''
            export UV_PROJECT_ENVIRONMENT="$VIRTUAL_ENV"
            uv run python -c "from amm_competition.cli import main; main()"
          '';
          best = '' cargo test -p optimizer -- list_top_20 --no-capture --ignored'';

          # Deploy to a local Anvil instance (assumes anvil is already running)
          deploy-local = ''
            set -e
            if ! command -v anvil &> /dev/null; then
              echo "❌ anvil not found. Make sure foundry is installed."
              exit 1
            fi
            # Check if anvil is running on port 8545
            if ! nc -z localhost 8545 2>/dev/null; then
              echo "No local node found at http://localhost:8545"
              echo "Start one with 'anvil' in another terminal."
              exit 1
            fi
            echo "Deploying to local Anvil..."
            # Example: replace with your actual deployment script path
            forge script script/Deploy.s.sol --rpc-url http://localhost:8545 --broadcast
          '';
          uv-venv = ''
            if [ -z "$VIRTUAL_ENV" ]; then
              echo "❌ VIRTUAL_ENV not set. Run 'uv venv' first."
              exit 1
            fi
            if [ ! -d "$VIRTUAL_ENV" ]; then
              uv venv --python 3.13 "$VIRTUAL_ENV"
            fi
            source "$VIRTUAL_ENV/bin/activate"
            uv pip install -e .
          '';
        };

        env = {
          FOUNDRY_PROFILE = "default";
          ETH_RPC_URL = "http://localhost:8545";
          RUST_BACKTRACE = "1";
        };

        shellHook = ''
          export VIRTUAL_ENV="${bash.wd}/.venv"
          ${bin.uv-venv}
          echo ""
          echo "🦀 Foundry development environment ready!"
          echo "Available commands: ${concatStringsSep ", " (attrNames scripts)}"
          echo "To start a local testnet: anvil"
          echo "To deploy to it: deploy-local (after anvil is running)"
          echo ""
        '';

      in
      {
        devShells.default = pkgs.mkShell {
          inherit shellHook;
          buildInputs = buildDeps ++ devDeps ++ (attrValues scripts);
          env = env;
        };
        rust.toolchain = rustToolchain;
        # _module.args = {
        #   pkgs = import nixpkgs {
        #     inherit system;
        #     overlays = [ (import rust-overlay) ];
        #   };
        # };
      };
  });
}
