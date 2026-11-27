//! Script files at the bound of `Script::read`: of a file with more than
//! `MAX_SCRIPT_BYTES` bytes only the metadata is read, and a file that
//! holds more bytes than its metadata states, as a named pipe does, is
//! refused once its bytes have been read.

use std::fs::File;
use std::path::{Path, PathBuf};

use kosmo_testkit::script::{MAX_SCRIPT_BYTES, Problem, Script};

/// The path of `name` in the directory that Cargo gives integration tests
/// for their files, without a file of an earlier run.
fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    // A file that an earlier run left, perhaps without permissions; there
    // is usually none.
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn a_script_file_larger_than_the_bound_is_not_read() {
    let path = scratch("too-large.script");
    // A sparse file: its length is set, and no byte is written.
    File::create(&path)
        .unwrap()
        .set_len(MAX_SCRIPT_BYTES + 1)
        .unwrap();
    let error = Script::read(&path).unwrap_err();
    assert_eq!(
        (error.line, error.problem),
        (0, Problem::TooLarge(MAX_SCRIPT_BYTES + 1))
    );
    // Only the metadata is read, so the file need not be readable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let error = Script::read(&path).unwrap_err();
        assert_eq!(
            (error.line, error.problem),
            (0, Problem::TooLarge(MAX_SCRIPT_BYTES + 1))
        );
    }
    std::fs::remove_file(&path).unwrap();
}

#[cfg(unix)]
#[test]
fn a_file_with_more_bytes_than_its_metadata_states_is_refused_once_read() {
    use std::io::Write;

    // A named pipe states no length, so `Script::read` reads what a writer
    // sends through it and counts the bytes once more.
    let path = scratch("more-than-stated.script");
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo: {made}");
    let bytes = vec![0; usize::try_from(MAX_SCRIPT_BYTES + 1).unwrap()];
    let writer = {
        let path = path.clone();
        std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new().write(true).open(&path)?;
            pipe.write_all(&bytes)
        })
    };
    let error = Script::read(&path).unwrap_err();
    assert_eq!(
        (error.line, error.problem),
        (0, Problem::TooLarge(MAX_SCRIPT_BYTES + 1))
    );
    // The reader read up to the end, so the writer has sent every byte.
    writer.join().unwrap().unwrap();
    std::fs::remove_file(&path).unwrap();
}
