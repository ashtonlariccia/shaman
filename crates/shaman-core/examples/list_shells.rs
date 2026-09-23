//! Print the shells detected on this machine.
//!
//! Handy for checking detection without launching the app:
//!   cargo.exe run -p shaman-core --example list_shells

fn main() {
    let profiles = shaman_core::profiles::detect();
    println!("detected {} shell(s):\n", profiles.len());
    for p in profiles {
        println!("  {:<14} {}", p.id, p.label);
        println!("  {:<14} {} {:?}\n", "", p.program, p.args);
    }
}
