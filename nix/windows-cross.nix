# Compile cfg(windows) with the native host compiler kept separate from MinGW.
{ pkgs, rust }:
let
  mingw = pkgs.pkgsCross.mingwW64.stdenv.cc;
in pkgs.mkShell {
  nativeBuildInputs = [ (rust.override { targets = [ "x86_64-pc-windows-gnu" ]; }) pkgs.stdenv.cc mingw pkgs.pkg-config ];
  CC = "${pkgs.stdenv.cc}/bin/cc";
  CXX = "${pkgs.stdenv.cc}/bin/c++";
  HOST_CC = "${pkgs.stdenv.cc}/bin/cc";
  CC_x86_64_unknown_linux_gnu = "${pkgs.stdenv.cc}/bin/cc";
  AR_x86_64_unknown_linux_gnu = "${pkgs.stdenv.cc.bintools}/bin/ar";
  CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "${mingw}/bin/x86_64-w64-mingw32-gcc";
  CC_x86_64_pc_windows_gnu = "${mingw}/bin/x86_64-w64-mingw32-gcc";
  CFLAGS_x86_64_pc_windows_gnu = "-I${pkgs.lib.getDev pkgs.pkgsCross.mingwW64.windows.pthreads}/include";
  AR_x86_64_pc_windows_gnu = "${mingw.bintools}/bin/x86_64-w64-mingw32-ar";
}
