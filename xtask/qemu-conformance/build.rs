//! Links the firmware for its QEMU machine: the `memory.x` of
//! `mps2-an505` or of `virt`, and the `link.x` of the runtime crate
//! (`cortex-m-rt` includes `memory.x` itself, `riscv-rt` takes it as a
//! script of its own). A host build (the stub) links nothing.

use std::path::PathBuf;

const MPS2_AN505: &str = "MEMORY
{
  FLASH : ORIGIN = 0x10000000, LENGTH = 4M
  RAM : ORIGIN = 0x38000000, LENGTH = 2M
}
";

const RISCV_VIRT: &str = "MEMORY
{
  RAM : ORIGIN = 0x80000000, LENGTH = 16M
}
REGION_ALIAS(\"REGION_TEXT\", RAM);
REGION_ALIAS(\"REGION_RODATA\", RAM);
REGION_ALIAS(\"REGION_DATA\", RAM);
REGION_ALIAS(\"REGION_BSS\", RAM);
REGION_ALIAS(\"REGION_HEAP\", RAM);
REGION_ALIAS(\"REGION_STACK\", RAM);
";

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=JOKER_CONFORMANCE_CASES");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if os != "none" {
        return Ok(());
    }
    let memory = match arch.as_str() {
        "arm" => MPS2_AN505,
        "riscv32" => RISCV_VIRT,
        other => return Err(format!("no QEMU machine for target_arch {other}")),
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").map_err(|e| e.to_string())?);
    std::fs::write(out.join("memory.x"), memory).map_err(|e| e.to_string())?;
    println!("cargo:rustc-link-search={}", out.display());
    if arch == "riscv32" {
        println!("cargo:rustc-link-arg-bins=-Tmemory.x");
    }
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    if let Ok(cases) = std::env::var("JOKER_CONFORMANCE_CASES") {
        println!("cargo:rerun-if-changed={cases}");
    }
    Ok(())
}
