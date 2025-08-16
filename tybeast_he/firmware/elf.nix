{
  lib,
  stdenv,
  rustPlatform,
  fetchFromGitHub,
  pkg-config,
  libudev-zero,
}:
rustPlatform.buildRustPackage rec {
  pname = "elf2uf2-rs";
  version = "2038e9a199101ee8a16d046a87136be2a607001d";

  src = fetchFromGitHub {
    owner = "JoNil";
    repo = pname;
    rev = "${version}";
    sha256 = "sha256-CHuTpnAnD4I5PY47sceSxfjGdPZznXWfARE4YBfYoIg=";
  };
  cargoHash = "sha256-42R8ZSPyUll7wWHFiDXY2Lpma1wWwF4VKRHTDt5CTwk=";
  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    libudev-zero
  ];

  cargoSha256 = "sha256-OefAKK5rvh4HSHd26Pac4BSbcNO3ntqBReYepulV8Dk=";
}
