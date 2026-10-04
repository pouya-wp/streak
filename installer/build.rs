// Embeds the freshly built streak.exe, the icon, version info and an asInvoker
// manifest (otherwise Windows' installer detection would force a UAC prompt).
include!("../src/icon.rs");

fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let exe = std::env::var("STREAK_EXE").map(std::path::PathBuf::from).unwrap_or_else(|_| root.join("target/release/streak.exe"));
    let exe = exe.canonicalize().unwrap_or_else(|_| panic!("build streak.exe first: {}", exe.display()));
    println!("cargo:rerun-if-changed={}", exe.display());
    println!("cargo:rerun-if-env-changed=STREAK_EXE");
    println!("cargo:rerun-if-changed=../src/icon.rs");
    println!("cargo:rustc-env=STREAK_EXE_PATH={}", exe.display());

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("setup.ico");
    std::fs::write(&ico, ico_file(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();
    let manifest = out.join("setup.manifest");
    std::fs::write(
        &manifest,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application>
  </compatibility>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
</assembly>"#,
    )
    .unwrap();
    let esc = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let version = env!("CARGO_PKG_VERSION");
    let v: Vec<&str> = version.split('.').collect();
    let rc = format!(
        r#"1 ICON "{ico}"
1 24 "{man}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Streak"
      VALUE "FileDescription", "Streak Setup"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Streak"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "StreakSetup.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = esc(&ico),
        man = esc(&manifest),
        a = v[0],
        b = v[1],
        c = v[2],
    );
    let rc_path = out.join("setup.rc");
    std::fs::write(&rc_path, rc).unwrap();
    let _ = embed_resource::compile(&rc_path, embed_resource::NONE);
}
