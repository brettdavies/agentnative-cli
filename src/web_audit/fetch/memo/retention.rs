//! What the memo keeps for later callers: the better of two answers, and a
//! body only while it fits under the per-body and per-audit caps. A body
//! past either cap still reaches the caller that read it; a later caller
//! that needs it asks again.

use super::MemoStats;
use crate::web_audit::fetch::hop::{self, HopRecord};

/// The largest body the memo keeps for later callers, and the cap on a body
/// it reads on its own. The site engine's value.
pub const MEMO_BODY_MAX_BYTES: usize = 1024 * 1024;
/// The most body bytes one audit's memo keeps. The site engine's value.
pub const MEMO_TOTAL_MAX_BYTES: usize = 8 * 1024 * 1024;

/// The record the memo keeps for later callers: the body only while it fits
/// under both caps.
pub(super) fn retained(record: &HopRecord, stats: &mut MemoStats) -> HopRecord {
    let HopRecord::Answer(answer) = record else {
        return record.clone();
    };
    let size = answer.body_len();
    if size > MEMO_BODY_MAX_BYTES {
        stats.over_body_cap += 1;
    } else if stats.retained_bytes + size > MEMO_TOTAL_MAX_BYTES {
        stats.over_total_cap += 1;
    } else {
        stats.retained_bytes += size;
        if size > 0 {
            stats.bodies_retained += 1;
        }
        stats.largest_body_bytes = stats.largest_body_bytes.max(size);
        return record.clone();
    }
    HopRecord::Answer(answer.without_body())
}

/// Whichever of the record on hand before a request went out and the one it
/// brought back serves more of the body; a tie goes to the new one.
pub(super) fn better_of(prior: Option<&HopRecord>, kept: HopRecord) -> HopRecord {
    match prior {
        Some(prior) if hop::coverage(prior) > hop::coverage(&kept) => prior.clone(),
        _ => kept,
    }
}
