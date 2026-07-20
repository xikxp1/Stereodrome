fn main() {
    println!("cargo:rerun-if-env-changed=LASTFM_API_KEY");
    println!("cargo:rerun-if-env-changed=LASTFM_SHARED_SECRET");
    tauri_build::build();
}
