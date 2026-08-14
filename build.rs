// Bakes the procedural icon + version info into streak.exe.
include!("src/icon.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("streak.ico");
    std::fs::write(&ico, ico_file(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    let v: Vec<&str> = version.split('.').collect();
    let rc = format!(
        r#"1 ICON "{ico}"
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
      VALUE "FileDescription", "Streak - cursor effects"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Streak"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "Streak.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = ico.display().to_string().replace('\\', "\\\\"),
        a = v[0],
        b = v[1],
        c = v[2],
    );
    let rc_path = out.join("streak.rc");
    std::fs::write(&rc_path, rc).unwrap();
    let _ = embed_resource::compile(&rc_path, embed_resource::NONE);
}
