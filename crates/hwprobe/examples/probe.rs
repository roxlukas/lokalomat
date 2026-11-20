//! Prints the hardware profile and memory budget as JSON:
//! `cargo run -p lokalsi-hwprobe --example probe`

fn main() {
    let profile = lokalsi_hwprobe::probe();
    let budget = lokalsi_hwprobe::budget(&profile, 70);
    println!("{}", serde_json::to_string_pretty(&profile).unwrap());
    println!("{}", serde_json::to_string_pretty(&budget).unwrap());
}
