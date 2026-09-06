//! Evidence for a play-now jump, independent of HTTP and Spotify credentials.
//! A raw queue is playlist continuation PLUS explicit items. Only an observed
//! insertion of our target establishes the end of the explicit prefix.

/// Locate a confirmed insertion, allowing Spotify's fixed-size window to drop
/// one tail item. Unchanged, reordered, or shortened snapshots prove nothing.
/// Adjacent identical copies can straddle the explicit/playlist boundary.
/// Reject ambiguous insertions instead of guessing which occurrence was added.
pub(crate) fn insertion(before: &[&str], after: &[&str], target: &str) -> Option<usize> {
    if before == after || after.len() < before.len() || after.len() > before.len() + 1 {
        return None;
    }
    let mut candidates = (0..=before.len()).filter(|&i| {
        i < after.len()
            && (after.len() > before.len() || i + 1 < after.len())
            && after[i] == target
            && before[..i] == after[..i]
            && before[i..]
                .iter()
                .take(after.len() - i - 1)
                .eq(after[i + 1..].iter())
    });
    let first = candidates.next()?;
    if candidates.next().is_some() {
        None
    } else {
        Some(first)
    }
}

/// On a successful landing, replace ONE pre-existing explicit target with the
/// new played copy. Preserve all other occurrences; playlist copies past the
/// insertion are never visited or restored.
pub(crate) fn restore_indices(prefix: &[&str], target: &str) -> Vec<usize> {
    let replaced = prefix.iter().position(|uri| *uri == target);
    (0..prefix.len()).filter(|i| Some(*i) != replaced).collect()
}

/// A URI alone cannot confirm a transition between two copies of one song.
/// Require the expected remaining snapshot to advance as well. Truncated API
/// windows may grow at the tail, so compare only the known remainder.
pub(crate) fn landed(
    current: Option<&str>,
    queue: &[&str],
    expected: &str,
    remaining: &[&str],
    previous: Option<&str>,
    previous_queue: &[&str],
) -> bool {
    current == Some(expected)
        && queue.starts_with(remaining)
        && (previous != current || queue != previous_queue)
}

/// Count only intermediate songs whose departure has been observed.
pub(crate) fn consumed_prefix(step: usize, left_previous: bool) -> usize {
    if left_previous {
        step
    } else {
        step.saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_target_is_promoted_without_visiting_playlist_songs() {
        // Explicit E, followed by playlist A, B, T. T is NOT explicitly queued.
        let before = ["E", "A", "B", "T"];
        let after = ["E", "T", "A", "B", "T"];
        let end = insertion(&before, &after, "T").unwrap();
        assert_eq!(end, 1);
        assert_eq!(restore_indices(&after[..end], "T"), vec![0]);
        assert_eq!(&after[end + 1..], &["A", "B", "T"]);
    }

    #[test]
    fn unchanged_delayed_snapshot_cannot_trigger_old_long_jump() {
        assert_eq!(
            insertion(&["E", "A", "B", "T"], &["E", "A", "B", "T"], "T"),
            None
        );
    }

    #[test]
    fn an_existing_explicit_copy_is_replaced_not_duplicated() {
        let before = ["A", "T", "B", "P"];
        let after = ["A", "T", "B", "T", "P"];
        let end = insertion(&before, &after, "T").unwrap();
        assert_eq!(end, 3);
        assert_eq!(restore_indices(&after[..end], "T"), vec![0, 2]);
    }

    #[test]
    fn intentional_duplicates_keep_their_multiplicity() {
        assert_eq!(
            insertion(&["A", "T", "T", "P"], &["A", "T", "T", "T", "P"], "T"),
            None
        );
        assert_eq!(restore_indices(&["A", "T", "T"], "T"), vec![0, 2]);
    }

    #[test]
    fn fixed_window_and_empty_queue() {
        assert_eq!(insertion(&["A", "B", "C"], &["T", "A", "B"], "T"), Some(0));
        assert_eq!(insertion(&[], &["T"], "T"), Some(0));
        assert_eq!(insertion(&["A"], &["A", "T"], "T"), Some(1));
    }

    #[test]
    fn concurrent_queue_edits_are_not_an_insertion() {
        assert_eq!(
            insertion(&["A", "B", "C"], &["B", "T", "A", "C"], "T"),
            None
        );
        assert_eq!(insertion(&["A", "B", "C"], &["T", "C"], "T"), None);
        assert_eq!(insertion(&["A"], &["T", "B", "A"], "T"), None);
    }

    #[test]
    fn repeated_uri_needs_evidence_of_queue_advancement() {
        assert!(!landed(
            Some("T"),
            &["T", "P"],
            "T",
            &["P"],
            Some("T"),
            &["T", "P"]
        ));
        assert!(landed(
            Some("T"),
            &["P"],
            "T",
            &["P"],
            Some("T"),
            &["T", "P"]
        ));
        assert!(!landed(Some("T"), &[], "T", &[], Some("T"), &[]));
    }

    #[test]
    fn wrong_landing_or_changed_remainder_stops_the_jump() {
        assert!(!landed(
            Some("X"),
            &["P"],
            "T",
            &["P"],
            Some("A"),
            &["T", "P"]
        ));
        assert!(!landed(
            Some("T"),
            &["X"],
            "T",
            &["P"],
            Some("A"),
            &["T", "P"]
        ));
        assert!(landed(
            Some("T"),
            &["P", "Q"],
            "T",
            &["P"],
            Some("A"),
            &["T", "P"]
        ));
    }
    #[test]
    fn continuation_tail_replacement_is_not_insertion_evidence() {
        assert_eq!(insertion(&["A", "B", "C"], &["A", "B", "T"], "T"), None);
    }
    #[test]
    fn interrupted_jump_restores_every_confirmed_departure() {
        assert_eq!(consumed_prefix(0, false), 0);
        assert_eq!(consumed_prefix(1, false), 0); // A may still be playing
        assert_eq!(consumed_prefix(1, true), 1); // observed X means A was left
        assert_eq!(consumed_prefix(2, true), 2);
    }
}
