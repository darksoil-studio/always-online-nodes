{ inputs, self, ... }:

{
  perSystem = { inputs', pkgs, self', lib, ... }: rec {

    packages.always-online-node = let
      craneLib = inputs.crane.mkLib pkgs;

      cratePath = ./.;

      cargoToml =
        builtins.fromTOML (builtins.readFile "${cratePath}/Cargo.toml");
      crate = cargoToml.package.name;

      commonArgs = {
        src = craneLib.cleanCargoSource (craneLib.path self.outPath);
        doCheck = false;
        buildInputs = self'.dependencies.holochain.buildInputs;
      };
    in craneLib.buildPackage (commonArgs // {
      pname = crate;
      version = cargoToml.package.version;
    });

    builders.aon-for-dna = { dna_bundle }:
      pkgs.runCommandLocal "aon-for-${dna_bundle}" {
        buildInputs = [ pkgs.makeWrapper ];
      } ''
        mkdir $out
        mkdir $out/bin
        makeWrapper ${packages.always-online-node}/bin/always-online-node $out/bin/always-online-node \
          --add-flags "${dna_bundle}"
      '';

  };
}
