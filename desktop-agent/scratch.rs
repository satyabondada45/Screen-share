use wry::WebViewBuilder; fn main() { WebViewBuilder::new().with_ipc_handler(|x| {}); }
