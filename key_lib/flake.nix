{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    rust-overlay,
    nixpkgs,
  }: let
    overlays = [(import rust-overlay)];
    pkgs = import nixpkgs {
      system = "x86_64-linux";
      inherit overlays;
    };
  in {
    devShell.x86_64-linux = pkgs.mkShell {
      buildInputs = [
        (pkgs.rust-bin.selectLatestNightlyWith (toolchain:
          toolchain.default.override {
            targets = ["thumbv6m-none-eabi"];
            extensions = ["rust-src" "rust-analyzer"];
          }))
        pkgs.flip-link
        pkgs.probe-rs
        pkgs.rustfmt
        pkgs.gcc-arm-embedded
      ];
      NUM_KEYS = 42;
      NUM_CONFIGS = 3;
      NUM_LAYERS = 6;
    };
  };
}
