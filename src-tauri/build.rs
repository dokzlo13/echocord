fn main() {
    // generate_context! must rerun when the embedded UI or notices change.
    println!("cargo:rerun-if-changed=../ui/dist");
    tauri_build::build();
}
