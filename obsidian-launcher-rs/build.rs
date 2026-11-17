//! Windows only: gives Lumos.exe its icon and file details.
fn main() {
    println!("cargo:rerun-if-changed=assets/lumos.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/lumos.ico")
            .set("ProductName", "Lumos Client")
            .set("FileDescription", "Lumos Client")
            .set("CompanyName", "Lumos Client");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Couldn't add the Windows icon: {e}");
        }
    }
}
