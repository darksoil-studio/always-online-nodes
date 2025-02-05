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

    builders.aon-for-happ = { happ_bundle }:
      pkgs.runCommandLocal "always-online-node" {
        buildInputs = [ pkgs.makeWrapper ];
      } ''
        mkdir $out
        mkdir $out/bin
        makeWrapper ${packages.always-online-node}/bin/always-online-node $out/bin/always-online-node \
          --add-flags "${happ_bundle}"
      '';

  };
}
