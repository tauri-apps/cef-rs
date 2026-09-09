with builtins; let
  crate-version = (
    fromTOML (
      readFile ../../Cargo.toml
    )
  ).workspace.package.version;
  version = elemAt (split "\\+" crate-version) 2;

  index = fromJSON (
    readFile (
      fetchurl "https://cef-builds.spotifycdn.com/index.json"
    )
  );

  splitCefVersion = version: split ''\+(g|chromium-)'' version;
  matchVersion = attrs: let matches = splitCefVersion attrs.cef_version; in version == (head matches);
  x64Versions = filter matchVersion index.linux64.versions;
  x64Version = head x64Versions;
  arm64Versions = filter matchVersion index.linuxarm64.versions;
  arm64Version = head arm64Versions;

  versionSegments = splitCefVersion x64Version.cef_version;
  gitRevision = elemAt versionSegments 2;
  chromiumVersion = elemAt versionSegments 4;

  matchType = attrs: "minimal" == attrs.type;
  x64Archive = head (filter matchType x64Version.files);
  arm64Archive = head (filter matchType arm64Version.files);

  sha1Hashes = {
    x86_64-linux = "${x64Archive.sha1}";
    aarch64-linux = "${arm64Archive.sha1}";
  };
in
{
  inherit
    version
    gitRevision
    chromiumVersion
    sha1Hashes;
}
