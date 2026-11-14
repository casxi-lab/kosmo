//! The fetch step: downloads the packages that every workspace of the check
//! needs for each of its target platforms, as its `Cargo.lock` records them.
//!
//! For each workspace that the layout table lists with crates, and for the
//! tool workspaces, the step runs
//! `cargo fetch --locked --manifest-path <workspace>/Cargo.toml --target <target>`
//! once for each target of the workspace in the table, in the order of
//! [`Index::checked`] and of the table. It is the only step of the check
//! that uses the network: the Cargo commands of the later steps run with
//! `CARGO_NET_OFFLINE=true`. Every command runs, also after one fails, and
//! the step fails with each failure.

use std::path::Path;

use crate::check::{self, Cmd, Facts};
use crate::layout::{self, Checked, Index};
use crate::repo::Tree;

/// The fetch step.
pub fn run(root: &Path, facts: &mut Facts) -> Result<(), String> {
    let tree = Tree::load(root)?;
    let table = layout::table(&tree)?;
    let index = Index::new(&table);
    check::run_each(root, &commands(&index.checked()), facts)
}

/// The commands of the fetch step for `workspaces`: one for each workspace
/// and target, with its time recorded as `<workspace>_<target>_ms`.
pub fn commands(workspaces: &[Checked]) -> Vec<Cmd> {
    workspaces
        .iter()
        .flat_map(|workspace| {
            workspace.targets.iter().map(|target| Cmd {
                program: check::cargo(),
                args: vec![
                    "fetch".to_string(),
                    "--locked".to_string(),
                    "--manifest-path".to_string(),
                    format!("{}/Cargo.toml", workspace.dir),
                    "--target".to_string(),
                    target.clone(),
                ],
                env: Vec::new(),
                timing: format!("{}_{target}_ms", workspace.dir),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::check::Facts;
    use crate::deps::fixture::{self, DARWIN, LINUX, STALE_LOCK, in_scratch, scratch};

    /// The arguments and the timing key of each command for the model's
    /// table.
    fn commands_of(model: &fixture::Model) -> Vec<(String, String)> {
        let tree = model.tree();
        let table = layout::table(&tree).unwrap();
        commands(&Index::new(&table).checked())
            .into_iter()
            .map(|cmd| {
                assert!(cmd.env.is_empty(), "the fetch step may use the network");
                (cmd.args.join(" "), cmd.timing)
            })
            .collect()
    }

    #[test]
    fn fetches_each_workspace_for_each_of_its_targets() {
        let expected: Vec<(String, String)> = ["xtask", "desktop"]
            .into_iter()
            .flat_map(|dir| {
                [LINUX, DARWIN].map(|target| {
                    (
                        format!(
                            "fetch --locked --manifest-path {dir}/Cargo.toml --target {target}"
                        ),
                        format!("{dir}_{target}_ms"),
                    )
                })
            })
            .collect();
        assert_eq!(commands_of(&fixture::desktop_only()), expected);
    }

    #[test]
    fn covers_the_tools_and_the_product_workspaces_with_crates() {
        let dirs: Vec<String> = commands_of(&fixture::valid())
            .into_iter()
            .map(|(_, timing)| timing)
            .collect();
        let expected: Vec<String> = ["xtask", "display", "framework", "desktop"]
            .into_iter()
            .flat_map(|dir| [LINUX, DARWIN].map(|target| format!("{dir}_{target}_ms")))
            .collect();
        assert_eq!(dirs, expected);
    }

    /// The commands of the fetch step for the workspace `ws` of a scratch
    /// directory, for `targets`.
    fn scratch_commands(dir: &std::path::Path, targets: &[String]) -> Vec<Cmd> {
        let workspace = Checked {
            dir: "ws",
            targets,
            lock_allowlist: "ws/allowlist.toml",
            product: None,
        };
        commands(&[workspace])
            .into_iter()
            .map(|cmd| in_scratch(dir, cmd))
            .collect()
    }

    #[test]
    fn a_stale_lock_file_fails_without_changing_it() {
        let dir = scratch("fetch-stale", &STALE_LOCK);
        let before = fs::read(dir.join("ws/Cargo.lock")).unwrap();
        let mut facts = Facts::new();
        let error = check::run_each(
            &dir,
            &scratch_commands(&dir, &[LINUX.to_string()]),
            &mut facts,
        )
        .unwrap_err();
        assert!(error.contains("exited with exit status: 101"), "{error}");
        assert!(error.contains("--locked was passed"), "{error}");
        assert_eq!(fs::read(dir.join("ws/Cargo.lock")).unwrap(), before);
        assert_eq!(facts.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_target_fails_its_command() {
        let files = [
            STALE_LOCK[0],
            STALE_LOCK[1],
            STALE_LOCK[2],
            STALE_LOCK[3],
            STALE_LOCK[4],
            (
                "ws/Cargo.lock",
                "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.1.0\"\n",
            ),
        ];
        let dir = scratch("fetch-target", &files);
        let targets = [LINUX.to_string(), "aarch64-unknown-linux-gun".to_string()];
        let mut facts = Facts::new();
        let error =
            check::run_each(&dir, &scratch_commands(&dir, &targets), &mut facts).unwrap_err();
        assert!(
            error.contains("--target aarch64-unknown-linux-gun` exited"),
            "{error}"
        );
        assert!(!error.contains(&format!("--target {LINUX}`")), "{error}");
        assert!(
            error.contains("could not find specification for target"),
            "{error}"
        );
        let keys: Vec<&str> = facts.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "ws_aarch64-unknown-linux-gnu_ms",
                "ws_aarch64-unknown-linux-gun_ms"
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
