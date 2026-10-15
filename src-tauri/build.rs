fn main() {
    #[cfg(windows)]
    {
        // The app manifest is embedded by `embed_app_manifest` below instead of tauri-build,
        // so that unit-test binaries get it too.
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        let attrs = tauri_build::Attributes::new().windows_attributes(windows);
        if let Err(e) = tauri_build::try_build(attrs) {
            panic!("tauri-build failed: {e:#}");
        }
        embed_app_manifest();
    }
    #[cfg(not(windows))]
    tauri_build::build();
}

/// Common-Controls v6 manifest (same content tauri-build uses by default).
/// tauri-build only links it into the app binary; unit-test binaries link the same
/// Tauri/dialog code and fail to start (STATUS_ENTRYPOINT_NOT_FOUND) without it.
/// `compile_for_everything` links it into every binary, including tests.
#[cfg(windows)]
fn embed_app_manifest() {
    use std::path::PathBuf;

    const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>"#;

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into()));
    let manifest_path = out.join("app.manifest");
    let rc_path = out.join("app-manifest.rc");
    let manifest_rc = manifest_path.display().to_string().replace('\\', "/");
    std::fs::write(&manifest_path, MANIFEST).unwrap_or_else(|e| panic!("write manifest: {e}"));
    std::fs::write(&rc_path, format!("1 24 \"{manifest_rc}\"\n"))
        .unwrap_or_else(|e| panic!("write rc: {e}"));
    if let Err(e) =
        embed_resource::compile_for_everything(&rc_path, embed_resource::NONE).manifest_required()
    {
        panic!("embedding app manifest failed: {e}");
    }
}
