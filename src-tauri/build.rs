fn main() {
    println!("cargo:rerun-if-changed=../dist/ui");
    tauri_build::build()
}
