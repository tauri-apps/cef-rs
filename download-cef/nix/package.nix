{
  stdenv,
  cef-binary,
  version ? cef-binary.version,
  gitRevision ? cef-binary.gitRevision,
  chromiumVersion ? cef-binary.chromiumVersion,
  sha1Hashes ? cef-binary.srcHashes,
}:
with builtins; let
  selectSystem = attrs: attrs.${stdenv.hostPlatform.system}
    or (throw "Unsupported system ${stdenv.hostPlatform.system}");

  cefArch = selectSystem {
    aarch64-linux = "linuxarm64";
    x86_64-linux = "linux64";
  };
  filename = "cef_binary_${version}+g${gitRevision}+chromium-${chromiumVersion}_${cefArch}_minimal.tar.bz2";
  src = fetchurl "https://cef-builds.spotifycdn.com/${filename}";
  srcHashes = {
    ${stdenv.hostPlatform.system} = convertHash {
      hash = (hashFile "sha256" src);
      hashAlgo = "sha256";
      toHashFormat = "sri";
    };
  };
  cef = cef-binary.override {
    inherit
      version
      gitRevision
      chromiumVersion
      srcHashes;
  };

  releaseDir = "${cef}/Release";
  linkReleaseDir = dir: ''ln -s "${releaseDir}/${dir}" "$out/${dir}"'';
  resourceDir = "${cef}/Resources";
  linkResourceDir = dir: ''ln -s "${resourceDir}/${dir}" "$out/${dir}"'';
  linkOther = dir: ''ln -s "${cef}/${dir}" "$out/${dir}"'';
  linkEverything = concatStringsSep "\n" (
    concatLists [
      (
        map linkReleaseDir (
          attrNames (
            readDir releaseDir
          )
        )
      )
      (
        map linkResourceDir (
          attrNames (
            readDir resourceDir
          )
        )
      )
      (
        map linkOther [
          "CMakeLists.txt"
          "cmake"
          "include"
          "libcef_dll"
          "CREDITS.html"
        ]
      )
    ]
  );

  archiveJson = toJSON {
    type = "minimal";
    name = filename;
    sha1 = selectSystem sha1Hashes;
  };
  archiveFile = toFile "archive.json" archiveJson;
in
stdenv.mkDerivation {
  pname = "export-cef-dir";
  inherit version;
  dontStrip = true;
  dontPatchELF = true;
  src = cef;
  buildInputs = [
    cef
  ];
  installPhase = ''
    runHook preInstall

    mkdir -p $out
    cp ${archiveFile} $out/archive.json
    ${linkEverything}

    runHook postInstall
  '';
}
