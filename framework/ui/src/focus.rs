//! Platform events as the round function takes them, and the focus check
//! in front of dispatch.

use std::collections::BTreeMap;
use std::fmt;

use kosmo_types::KeyEvent;

// The identity of a surface and the focus stamp belong to the platform
// interface, which reports them; the round function takes them as they are.
pub use kosmo_platform::{Stamp, SurfaceId};

/// A platform event of a round, in the order the platform delivered it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A delivered key event for a surface, with its focus stamp.
    Key {
        /// The surface.
        surface: SurfaceId,
        /// The focus stamp.
        stamp: Stamp,
        /// The key event.
        event: KeyEvent,
    },
    /// The surface gained focus with this stamp.
    FocusIn {
        /// The surface.
        surface: SurfaceId,
        /// The focus stamp.
        stamp: Stamp,
    },
    /// The surface lost the focus that it gained with this stamp.
    FocusOut {
        /// The surface.
        surface: SurfaceId,
        /// The focus stamp.
        stamp: Stamp,
    },
}

/// Why the focus check dropped a key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Dropped {
    /// Its epoch differs from the epoch of the surface's latest focus in.
    Epoch,
    /// Its focus generation differs from the generation of the surface's
    /// latest focus in.
    Generation,
    /// Its surface does not hold focus.
    NoFocus,
}

impl fmt::Display for Dropped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Dropped::Epoch => "epoch",
            Dropped::Generation => "generation",
            Dropped::NoFocus => "no-focus",
        })
    }
}

/// For each surface, the stamp of its latest focus in and whether it holds
/// focus.
#[derive(Debug, Default)]
pub(crate) struct FocusTable {
    surfaces: BTreeMap<SurfaceId, (Stamp, bool)>,
}

impl FocusTable {
    /// The surface gained focus with `stamp`.
    pub(crate) fn enter(&mut self, surface: SurfaceId, stamp: Stamp) {
        self.surfaces.insert(surface, (stamp, true));
    }

    /// The surface lost the focus of `stamp`. A focus out with another
    /// stamp than the latest focus in is stale and changes nothing; the
    /// result says whether it applied.
    pub(crate) fn leave(&mut self, surface: SurfaceId, stamp: Stamp) -> bool {
        match self.surfaces.get_mut(&surface) {
            Some((current, holds)) if *current == stamp => {
                *holds = false;
                true
            }
            _ => false,
        }
    }

    /// The stamp of the surface's latest focus in, and whether it holds
    /// focus.
    pub(crate) fn get(&self, surface: SurfaceId) -> Option<(Stamp, bool)> {
        self.surfaces.get(&surface).copied()
    }

    /// Checks the stamp of a key event for `surface`: its epoch, then its
    /// focus generation, then whether the surface holds focus.
    pub(crate) fn check(&self, surface: SurfaceId, stamp: Stamp) -> Result<(), Dropped> {
        let Some(&(current, holds)) = self.surfaces.get(&surface) else {
            return Err(Dropped::NoFocus);
        };
        if stamp.epoch != current.epoch {
            Err(Dropped::Epoch)
        } else if stamp.generation != current.generation {
            Err(Dropped::Generation)
        } else if holds {
            Ok(())
        } else {
            Err(Dropped::NoFocus)
        }
    }
}
