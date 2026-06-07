{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs = inputs@{ flake-parts, ... }: with builtins;
    flake-parts.lib.mkFlake { inherit inputs; } ({ ... }: {
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];

      perSystem = { pkgs, ... }:
        let
          # Base dependencies for building & development
          buildDeps = with pkgs; [
            foundry
            uv
            maturin
            rustc
          ];
          # Additional tools for the interactive shell
          devDeps = [ ];

          bash.wd = ''$(git rev-parse --show-toplevel 2>/dev/null || echo \".\")'';
          bin = mapAttrs (name: pkg: "${pkg}/bin/${name}") scripts;
          scripts = mapAttrs pkgs.writeShellScriptBin {
            fbuild = '' forge build --contracts contracts'';
            utest = '' forge test -vvv --contracts contracts'';
            simbuild = '' cd amm_sim_rs ; maturin develop --release ; source "$VIRTUAL_ENV/bin/activate" '';
            simtest = ''
              simbuild ; source "$VIRTUAL_ENV/bin/activate"
              amm-match run "${bash.wd}/contracts/src/StarterStrategy.sol" --simulations 10
            '';
            snap = '' forge snapshot '';
            cov = '' forge coverage '';
            clean = '' forge clean '';
            check = '' set -e
              ${bin.clean}
              ${bin.fbuild}
              ${bin.utest}
              ${bin.snap}
              echo "✅ All checks passed!"
            '';

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
            uv-venv = with bash; ''
              export VIRTUAL_ENV="${wd}/.cache/venv"
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
          };

          shellHook = ''
            export VIRTUAL_ENV="${bash.wd}/.cache/venv"
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
        };
    });
}
