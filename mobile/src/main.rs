//! iOS entry point, and a phone-sized desktop preview of the mobile app.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "ios")]
    {
        // The app sandbox: Documents is shown in the Files app when the
        // Info.plist sets UIFileSharingEnabled / LSSupportsOpeningDocumentsInPlace.
        let home = PathBuf::from(std::env::var("HOME")?);
        let data_dir = home.join("Library/Application Support/OpenBilling");
        let export_dir = home.join("Documents/OpenBilling");
        return openbilling::run(data_dir, export_dir, None);
    }

    #[cfg(not(target_os = "ios"))]
    {
        // Desktop preview: same code paths as the phone build.
        let data_dir = std::env::var("OPENBILLING_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("mobile-preview-data"));
        let export_dir = data_dir.join("exports");
        openbilling::run(data_dir, export_dir, Some((390.0, 844.0)))
    }
}
