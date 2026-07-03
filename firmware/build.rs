fn main() {
    // esp-hal's build script places `linkall.x` (and the memory-region scripts it
    // pulls in) on the linker search path; this pulls it into the final link.
    println!("cargo:rustc-link-arg=-Tlinkall.x");
}
