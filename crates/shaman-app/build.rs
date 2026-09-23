//! Embeds an application manifest so release builds request elevation.
//!
//! Release builds run elevated (one UAC prompt at launch), which is what makes
//! admin terminals open instantly afterwards. Debug builds deliberately stay
//! `asInvoker`: routine `cargo run` / `verify.sh` cycles would otherwise fire a
//! UAC prompt every single time. Elevation is tested by launching a build
//! elevated on purpose.
//!
//! ## Do not drop the Common-Controls dependency
//!
//! Supplying an `app_manifest` *replaces* tauri-build's default
//! (`windows-app-manifest.xml`) rather than merging with it. That default exists
//! to declare a dependency on **Microsoft.Windows.Common-Controls 6.0.0.0**.
//! Without it the process binds to comctl32 v5, which does not export
//! `TaskDialogIndirect` — and the app dies at startup with:
//!
//! ```text
//! The procedure entry point TaskDialogIndirect could not be located
//! in the dynamic link library
//! ```
//!
//! An earlier version of this file omitted it and shipped exactly that bug. The
//! block below is Tauri's default manifest with `trustInfo` added.

/// `requestedExecutionLevel` is the only thing that varies between profiles.
fn manifest(execution_level: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <!-- Required: comctl32 v6 exports TaskDialogIndirect, v5 does not. -->
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>

  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="{execution_level}" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>

  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 / 11 -->
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}" />
    </application>
  </compatibility>

  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#
    )
}

fn main() {
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let level = if release {
        "requireAdministrator"
    } else {
        "asInvoker"
    };
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:warning=embedding manifest with requestedExecutionLevel={level}");

    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(manifest(level)),
    );

    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
