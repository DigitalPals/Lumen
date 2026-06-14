{
  lib,
  src,
  fftw,
  glib,
  gtk4-layer-shell,
  gtksourceview5,
  installShellFiles,
  libpulseaudio,
  libxkbcommon,
  pipewire,
  pixman,
  pkg-config,
  rustPlatform,
  stdenv,
  udev,
  wrapGAppsHook4,
  desktop-file-utils,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "lumen";
  version = (builtins.fromTOML (builtins.readFile "${src}/Cargo.toml")).workspace.package.version;

  inherit src;

  __structuredAttrs = true;
  strictDeps = true;

  cargoLock.lockFile = "${src}/Cargo.lock";

  nativeBuildInputs = [
    desktop-file-utils
    glib
    installShellFiles
    pkg-config
    rustPlatform.bindgenHook
    wrapGAppsHook4
  ];

  buildInputs = [
    fftw.dev
    gtk4-layer-shell.dev
    gtksourceview5
    libpulseaudio
    libxkbcommon.dev
    pipewire.dev
    pixman
    udev
  ];

  cargoBuildFlags = [
    "--bin=lumen"
    "--bin=lumen-settings"
  ];

  env.LUMEN_RESOURCES_DIR = "${placeholder "out"}/share/lumen/icons/hicolor/scalable/actions";
  env.CARGO_PROFILE_RELEASE_LTO = "false";
  env.CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16";

  # Keep package builds focused on producing binaries. The release profile uses
  # LTO, so compiling test binaries here roughly doubles local build time.
  doCheck = false;

  postInstall =
    ''
      install -Dm0644 resources/com.lumen.settings.desktop \
        "$out/share/applications/com.lumen.settings.desktop"
      desktop-file-validate "$out/share/applications/com.lumen.settings.desktop"

      install -Dm0644 resources/lumen-settings.svg \
        "$out/share/icons/hicolor/scalable/apps/lumen-settings.svg"

      mkdir -p "$out/share/lumen"
      cp -r resources/icons "$out/share/lumen/icons"

      mkdir -p "$out/share/icons"
      cp -r resources/icons/hicolor/. "$out/share/icons/hicolor/"

      install -Dm0644 resources/lumen.service "$out/share/systemd/user/lumen.service"
      substituteInPlace "$out/share/systemd/user/lumen.service" \
        --replace-fail "/usr/bin/lumen" "$out/bin/lumen"
    ''
    + lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
      installShellCompletion --cmd lumen \
        --bash <("$out/bin/lumen" completions bash) \
        --fish <("$out/bin/lumen" completions fish) \
        --zsh <("$out/bin/lumen" completions zsh)
    '';

  preFixup = ''
    gappsWrapperArgs+=( --suffix PATH : "$out/bin" )
    gappsWrapperArgs+=( --set-default LUMEN_RESOURCES_DIR "$out/share/lumen/icons/hicolor/scalable/actions" )
  '';

  meta = {
    description = "Wayland desktop shell with a bar, notifications, OSD, wallpaper, and device controls";
    homepage = "https://lumen.app/";
    license = lib.licenses.mit;
    mainProgram = "lumen";
    platforms = lib.platforms.linux;
  };
})
