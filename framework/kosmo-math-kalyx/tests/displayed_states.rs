//! States: a state line shows the name of its state. No operation of kalyx
//! 0.1.0 ends in a state, so the line is the one that kalyx 0.1.0's own
//! tests assert for `IterationLimit` under the versions 1, 2 and 3.

mod support;

use kosmo_math_kalyx::read_output_line;
use kosmo_math_model::display::{Content, Freshness};
use kosmo_math_model::version::{ContextVersion, SettingsVersion, Versions};
use support::{Checked, line};

#[test]
fn a_state_line_shows_the_name_of_its_state() -> Checked {
    // A line typed as one insertion has the source version 1.
    let snapshot = line("1+2")?.snapshot();
    let versions = Versions {
        source: snapshot.version(),
        context: ContextVersion::new(2),
        settings: SettingsVersion::new(3),
    };
    assert_eq!(versions.source.get(), 1);
    let item = read_output_line(b"state\t1\t2\t3\tIterationLimit\n", versions, &snapshot);
    assert_eq!(item.content, Content::State("IterationLimit".to_string()));
    assert_eq!(
        item.render(Freshness::Current),
        "state (source 1, settings 3): IterationLimit"
    );
    Ok(())
}
