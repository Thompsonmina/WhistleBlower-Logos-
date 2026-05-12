{
  description = "Whistleblower — LP-17 basecamp app + Chronicle indexing module + on-chain registry";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    nixpkgs.follows = "logos-module-builder/nixpkgs";

    # Carried over from logos-chronicle/flake.nix.
    nix-bundle-lgx.url = "github:logos-co/nix-bundle-lgx";
    storage_module.url = "github:logos-co/logos-storage-module";
    storage_module.inputs.logos-module-builder.follows = "logos-module-builder";
    delivery_module.url = "github:logos-co/logos-delivery-module";
    delivery_module.inputs.logos-module-builder.follows = "logos-module-builder";
  };

  outputs = inputs@{ self, nixpkgs, logos-module-builder, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      # Path to the pre-built FFI .so. Built outside nix via `make ffi` (which
      # runs `cargo build --release` in `ffi/` and stages the artifact here).
      # If this file doesn't exist, flake eval fails with "no such file" —
      # informative error pointing at the prereq.
      ffiSo = ./logos-chronicle/vendored/libchronicle_registry_ffi.so;

      chronicleMod = logos-module-builder.lib.mkLogosModule {
        src = ./logos-chronicle;
        configFile = ./logos-chronicle/metadata.json;
        flakeInputs = inputs;
        postInstall = ''
          mkdir -p $out/lib
          cp ${ffiSo} $out/lib/libchronicle_registry_ffi.so
          echo "chronicle: bundled FFI -> $out/lib/libchronicle_registry_ffi.so"
        '';
      };

      whistleblowerMod = logos-module-builder.lib.mkLogosQmlModule {
        src = ./logos-whistleblower;
        configFile = ./logos-whistleblower/metadata.json;
        flakeInputs = inputs // { chronicle = chronicleMod; };
      };

      # Flatten per-system packages, prefixed so they don't collide.
      packagesFor = system:
        let
          chronicleHere = chronicleMod.packages.${system} or {};
          whistleHere = whistleblowerMod.packages.${system} or {};
          prefix = pfx: set: nixpkgs.lib.mapAttrs'
            (n: v: nixpkgs.lib.nameValuePair "${pfx}-${n}" v) set;
        in
        prefix "chronicle" chronicleHere
        // prefix "whistleblower" whistleHere;
    in
    {
      packages = nixpkgs.lib.genAttrs systems packagesFor;

      devShells = chronicleMod.devShells or {};
      checks = chronicleMod.checks or {};
    };
}
