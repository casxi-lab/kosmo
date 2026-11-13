//! `kosmo --version` prints the program name and the package version.

use std::process::Command;

#[test]
fn prints_the_name_and_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_kosmo"))
        .arg("--version")
        .output()
        .expect("kosmo runs");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("kosmo {}\n", env!("CARGO_PKG_VERSION"))
    );
}
