//! Version marks: a result carries the versions of its request. Once the
//! settings version or the source version of the region has changed, the
//! result is stale and rendered as such, never as the current result; while
//! the versions are unchanged, it is current.
//!
//! Whether a result is current is judged here from the three versions
//! alone. The judgement of the job layer and the result slot also compares
//! submissions and is not part of the display model.

mod support;

use kalyx::command::serialize;
use kalyx::{Limits, Request, execute};
use kosmo_math_kalyx::{convert, read_output_line};
use kosmo_math_model::display::{DisplayItem, Freshness};
use kosmo_math_model::version::{SettingsVersion, Versions};
use support::{Checked, context, line};

/// How a result of the versions `result` stands against the current
/// versions of the region.
fn freshness(result: Versions, current: Versions) -> Freshness {
    if result == current {
        Freshness::Current
    } else {
        Freshness::Stale
    }
}

/// The line `1/3+1/6`, typed as one insertion, and the display item of the
/// result of its submission under the context and settings versions 0.
fn submitted() -> Checked<(kosmo_math_model::EditBuffer, DisplayItem)> {
    let buffer = line("1/3+1/6")?;
    let snapshot = buffer.snapshot();
    let versions = Versions {
        source: snapshot.version(),
        ..Versions::default()
    };
    assert_eq!(versions.source.get(), 1);
    let request = Request::new("eval", "", &convert(&snapshot)?)
        .with_input_version(versions.source.get())
        .with_context_version(versions.context.get())
        .with_settings_version(versions.settings.get());
    let mut context = context(Limits::new());
    let outcome = execute(&request, &mut context);
    let written = serialize(&outcome, &mut context)?;
    assert_eq!(written, b"value\t1\t0\t0\t1/2\n");
    let item = read_output_line(&written, versions, &snapshot);
    assert_eq!(item.versions, versions);
    let shown = item.render(freshness(item.versions, versions));
    assert_eq!(shown, "value (source 1, settings 0): 1/2");
    Ok((buffer, item))
}

#[test]
fn a_result_is_stale_once_the_settings_version_changed() -> Checked {
    let (_, item) = submitted()?;
    let current = Versions {
        settings: SettingsVersion::new(1),
        ..item.versions
    };
    let shown = item.render(freshness(item.versions, current));
    assert_eq!(shown, "value (source 1, settings 0, stale): 1/2");
    assert_ne!(shown, item.render(Freshness::Current));
    Ok(())
}

#[test]
fn a_result_is_stale_once_the_line_changed_again() -> Checked {
    let (mut buffer, item) = submitted()?;
    buffer.insert_char('+')?;
    let current = Versions {
        source: buffer.version(),
        ..item.versions
    };
    assert_eq!(current.source.get(), 2);
    let shown = item.render(freshness(item.versions, current));
    assert_eq!(shown, "value (source 1, settings 0, stale): 1/2");
    assert_ne!(shown, item.render(Freshness::Current));
    Ok(())
}
