fn main() {
    let num_configs = std::env::var("NUM_CONFIGS").expect("NUM_CONFIGS is not set"); // Default value
    println!("cargo:rerun-if-env-changed=NUM_CONFIGS");
    let num_keys = std::env::var("NUM_KEYS").expect("NUM_KEYS is not set"); // Default value
    println!("cargo:rerun-if-env-changed=NUM_KEYS");
    let num_layers = std::env::var("NUM_LAYERS").expect("NUM_LAYERS is not set");
    println!("cargo:rerun-if-env-changed=NUM_LAYERS");
    let contents = format!(
        r#"pub const NUM_CONFIGS: usize = {};
pub const NUM_KEYS: usize = {};
pub const NUM_LAYERS: usize = {};"#,
        num_configs, num_keys, num_layers,
    );
    std::fs::write("src/config.rs", contents).expect("Failed to write config.rs");
}
