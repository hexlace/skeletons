//! The poisoned sample the sync message tests share beyond what
//! [`crate::survey::poison`] holds, because it is built from a sync type.

use crate::survey::poison::poison;
use crate::sync::write::{Leftover, LeftoverReason};

/// A leftover that could not be removed, at a poisoned path and with a
/// poisoned system error.
pub(super) fn poisoned_leftover() -> Leftover {
    Leftover {
        path: poison(),
        reason: LeftoverReason::CouldNotRemove { detail: poison() },
    }
}
