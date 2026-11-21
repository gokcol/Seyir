# Seyir Microsoft Store package

The **Seyir Microsoft Store MSIX** workflow packages the selected Git ref on Windows. Choose **store** and enter the exact package name, publisher distinguished name, and publisher display name from Partner Center. The same values can come from GitHub repository variables named `MSIX_IDENTITY_NAME`, `MSIX_PUBLISHER`, and `MSIX_PUBLISHER_DISPLAY_NAME`. The display name defaults to Seyir and can be set in the workflow or via `MSIX_DISPLAY_NAME`.

The default MSIX version is the Cargo version with a trailing `.0`. Supply a higher four-part version with revision `0` when required by Store update ordering. Upload the unsigned `*-store-x64.msix` to the product's package submission in Partner Center. The workflow also supplies the SHA-256 checksum and the packaged source commit.

Test mode uses `Seyir.PackagingTest` and validates installation and removal using a locally signed copy on the disposable runner. The `*-TEST-ONLY-x64.msix` artifact is for packaging validation.

The package declares `runFullTrust` for the native Win32 desktop process, which opens and saves user-selected local files, supports clipboard and drag-and-drop operations, and invokes Windows printing. Explorer registration uses the file types decoded by the app and passes the selected path as a quoted argument. The icons in `Assets` are resized from `assets/seyir.png`.

To build a test package locally with Rust 1.93.1 and the Windows SDK installed, run:

    pwsh -File packaging/windows/bundle-msix.ps1 -TestIdentity
