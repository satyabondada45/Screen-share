extern crate winres;

fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set("ProductName", "DeskStream");
        res.set("FileDescription", "DeskStream Remote Desktop Agent");
        res.set("LegalCopyright", "Copyright (c) Friends Software Solutions");
        res.set("CompanyName", "Friends Software Solutions");
        res.set("InternalName", "desktop-agent.exe");
        res.set("OriginalFilename", "desktop-agent.exe");
        // We set the version directly as a string to avoid parsing issues.
        res.set("FileVersion", "1.1.3.0");
        res.set("ProductVersion", "1.1.3.0");
        
        // Note: In a production setting, you'd provide a .ico file.
        // res.set_icon("assets/icon.ico");

        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile Windows resources: {}", e);
        }
    }
}
