{ inputs, self, ... }:

{
  perSystem = { inputs', pkgs, self', lib, system, ... }: rec {

    packages.always-online-node = let
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain
        inputs'.holonix.packages.rust;

      cratePath = ./.;

      cargoToml =
        builtins.fromTOML (builtins.readFile "${cratePath}/Cargo.toml");
      crate = cargoToml.package.name;

      commonArgs = {
        src = craneLib.cleanCargoSource (craneLib.path self.outPath);
        doCheck = false;
        buildInputs =
          inputs.holochain-utils.outputs.dependencies.${system}.holochain.buildInputs;

        # Make sure libdatachannel can find C++ standard libraries from clang.
        LIBCLANG_PATH = "${pkgs.llvmPackages_18.libclang.lib}/lib";
      };
    in craneLib.buildPackage (commonArgs // {
      pname = crate;
      version = cargoToml.package.version;
    });

    builders.aon-for-happs = { happs }:
      pkgs.runCommandLocal "always-online-node" {
        buildInputs = [ pkgs.makeWrapper ];
      } ''
        mkdir $out
        mkdir $out/bin
        makeWrapper ${packages.always-online-node}/bin/always-online-node $out/bin/always-online-node \
          --add-flags "${lib.strings.concatStringsSep " " happs}"
      '';

    checks.aon-for-happs = let
      happ = inputs.holochain-utils.outputs.builders.${system}.happ {
        happManifest = builtins.toFile "happ.yaml" ''
          manifest_version: '1'
          name: happ-store
          description: null
          roles: []
          allow_deferred_memproofs: false
        '';
        dnas = { };
      };

    in builders.aon-for-happs { happs = [ happ ]; };
  };
}
