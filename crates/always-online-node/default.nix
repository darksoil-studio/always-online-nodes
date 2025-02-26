{ inputs, self, ... }:

{
  perSystem = { inputs', pkgs, self', lib, system, ... }: rec {

    packages.always-online-node = let
      craneLib = inputs.crane.mkLib pkgs;

      cratePath = ./.;

      cargoToml =
        builtins.fromTOML (builtins.readFile "${cratePath}/Cargo.toml");
      crate = cargoToml.package.name;

      commonArgs = {
        src = craneLib.cleanCargoSource (craneLib.path self.outPath);
        doCheck = false;
        buildInputs =
          inputs.tnesh-stack.outputs.dependencies.${system}.holochain.buildInputs;
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
      happ = inputs.tnesh-stack.outputs.builders.${system}.happ {
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
