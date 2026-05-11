{
  description = "Basecamp UI for publishing whistleblower documents through the Chronicle module";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    chronicle.url = "path:../logos-chronicle";
    chronicle.inputs.logos-module-builder.follows = "logos-module-builder";
  };

  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
