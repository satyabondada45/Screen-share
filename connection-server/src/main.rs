use screenshots::Screen;
use std::fs;
use std::time::Instant;

fn main() {
    println!("========================================");
    println!("       Screen Share - CAPTURE TEST      ");
    println!("========================================\n");

    // Grab all available display screens on the host machine
    let screens = Screen::all().expect("Failed to get screens");

    for (i, screen) in screens.iter().enumerate() {
        println!("Capturing Screen {}: {}x{}", i, screen.display_info.width, screen.display_info.height);
        
        let start = Instant::now();
        // Capture the screen image buffer
        let image = screen.capture().expect("Failed to capture screen");
        println!("Captured in {:?}", start.elapsed());

        // Save the raw buffer as a PNG file to verify it worked
        let buffer = image.to_png().expect("Failed to encode PNG");
        let filename = format!("test_screenshot_{}.png", i);
        fs::write(&filename, buffer).expect("Failed to write file");
        
        println!("Success! Saved to '{}'\n", filename);
    }

    println!("Capture test complete. Press Enter to exit...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}