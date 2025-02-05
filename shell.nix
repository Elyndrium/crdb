let
  pkgs = import ./nix;
in
pkgs.stdenv.mkDerivation {
  name = "crdb";
  buildInputs = (
    (with pkgs; [
      cargo-bolero
      cargo-nextest
      niv
      samply
      sqlx-cli
      trunk

      (fenix.combine (with fenix; [
        minimal.cargo
        minimal.rustc
        targets.wasm32-unknown-unknown.latest.rust-std
      ]))
    ])
  );
}
