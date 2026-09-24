extern crate winres;

fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set("ProductName", "DeskStream");
        res.set("FileDescription", "DeskStream Remote Desktop Agent");
        res.set("LegalCopyright", "Copyright (c) Friends Software Solutions");
        res.set("CompanyName", "Friends Software Solutions");
        res.set("InternalName", "DeskStream-Agent.exe");
        res.set("OriginalFilename", "DeskStream-Agent.exe");
        res.set("FileVersion", "1.1.3.0");
        res.set("ProductVersion", "1.1.3.0");

        // Embed the DeskStream app icon
        res.set_icon("assets/icon.ico");

        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile Windows resources: {}", e);
        }
    }
}
