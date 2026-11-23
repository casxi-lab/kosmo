//! Arguments other than none and the single `--version` get the usage on
//! standard error and the exit code 2, with nothing on standard output.

use std::process::Command;

#[test]
fn other_arguments_get_the_usage_and_exit_with_two() {
    for args in [&["-V"][..], &["--version", "--version"], &["--help"], &[""]] {
        let output = Command::new(env!("CARGO_BIN_EXE_kosmo"))
            .args(args)
            .output()
            .expect("kosmo runs");
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert_eq!(output.stderr, b"Usage: kosmo [--version]\n", "{args:?}");
    }
}
