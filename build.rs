fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=packaging/windows/seyir.rc");
        println!("cargo:rerun-if-changed=packaging/windows/Seyir.ico");
        embed_resource::compile("packaging/windows/seyir.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Could not embed the Seyir executable icon");
    }
}
