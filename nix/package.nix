{
  self,
  lib,
  rustPlatform,
}:
let
  fmtDate =
    raw:
    let
      year = builtins.substring 0 4 raw;
      month = builtins.substring 4 2 raw;
      day = builtins.substring 6 2 raw;
    in
    "${year}-${month}-${day}";
in
rustPlatform.buildRustPackage {
  pname = "iridion";
  version = "unstable-${fmtDate self.lastModifiedDate}-${self.shortRev or "dirty"}";

  src = lib.cleanSourceWith {
    filter =
      name: _:
      let
        baseName = baseNameOf (toString name);
      in
      !(lib.hasSuffix ".nix" baseName);
    src = lib.cleanSource ../.;
  };

  cargoLock.lockFile = ../Cargo.lock;
  doCheck = false;

  meta = {
    description = "Extract base16 color palettes from images using Oklch perceptual clustering";
    homepage = "https://github.com/hambosto/iridion";
    license = lib.licenses.mit;
    mainProgram = "iridion";
    platforms = lib.platforms.unix;
  };
}
