//! The conformance suite of the platform interface on the headless
//! backend: each requirement holds (one test per requirement), and for each
//! requirement a backend with a defect against it fails that requirement
//! and no other (one test per defect).

mod support;

use kosmo_testkit::conformance::{self, REQUIREMENTS};
use support::{Defective, headless};

/// One test per requirement, named after it, that checks it on the
/// headless backend.
macro_rules! holds {
    ($($name:ident = $n:literal,)*) => {
        $(
            #[test]
            fn $name() {
                if let Err(failure) = conformance::check($n, headless) {
                    panic!("{failure}");
                }
            }
        )*
        #[test]
        fn every_requirement_has_a_test() {
            assert_eq!(vec![$($n),*], (1..=REQUIREMENTS).collect::<Vec<usize>>());
        }
    };
}

/// One test per requirement that runs the whole suite on the headless
/// backend with the defect against that requirement, which must be the
/// only one to fail.
macro_rules! fails_alone {
    ($($name:ident = $n:literal,)*) => {
        $(
            #[test]
            fn $name() {
                let report = conformance::run(|setup| Defective::new(headless(setup), $n));
                assert_eq!(report.failing(), [$n], "\n{report}");
            }
        )*
        #[test]
        fn every_requirement_has_a_defect() {
            assert_eq!(vec![$($n),*], (1..=REQUIREMENTS).collect::<Vec<usize>>());
        }
    };
}

holds! {
    requirement_01_injected_events_come_back_in_order = 1,
    requirement_02_a_wait_ends_at_its_deadline_or_is_idle = 2,
    requirement_03_a_wait_returns_at_most_the_bound_of_events = 3,
    requirement_04_the_clock_reads_what_the_driver_advanced = 4,
    requirement_05_destroyed_surfaces_are_stale = 5,
    requirement_06_at_most_the_bound_of_surfaces = 6,
    requirement_07_two_surfaces_stay_apart = 7,
    requirement_08_a_buffer_reads_back_as_presented = 8,
    requirement_09_a_presented_buffer_is_busy_until_released = 9,
    requirement_10_at_most_the_bound_of_rectangles = 10,
    requirement_11_other_formats_are_unsupported = 11,
    requirement_12_resources_come_from_the_table = 12,
    requirement_13_tree_updates_apply_in_order = 13,
    requirement_14_at_most_the_bound_of_nodes = 14,
    requirement_15_at_most_the_bound_of_injected_events = 15,
    requirement_16_an_injected_disconnection_ends_the_wait = 16,
}

fails_alone! {
    defect_01_reversed_events = 1,
    defect_02_idle_at_the_deadline = 2,
    defect_03_one_event_over_the_bound = 3,
    defect_04_waits_advance_the_clock = 4,
    defect_05_presents_on_a_destroyed_surface = 5,
    defect_06_a_surface_over_the_bound = 6,
    defect_07_one_read_back_for_all_surfaces = 7,
    defect_08_reads_back_the_previous_presentation = 8,
    defect_09_hands_out_a_busy_buffer = 9,
    defect_10_accepts_a_rectangle_over_the_bound = 10,
    defect_11_accepts_an_unsupported_format = 11,
    defect_12_a_missing_resource_is_empty = 12,
    defect_13_keeps_only_the_last_update = 13,
    defect_14_changes_the_tree_over_the_bound = 14,
    defect_15_queues_injected_events_over_the_bound = 15,
    defect_16_drops_an_injected_disconnection = 16,
}
