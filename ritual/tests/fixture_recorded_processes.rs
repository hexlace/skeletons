//! Acceptance: the fixtures' process cleanup only ever signals single,
//! ordinary processes.
//!
//! [`support::recorded_processes::KillRecordedProcesses`] reads process ids
//! from a file a test's helper script wrote. `kill -KILL 0` signals the
//! caller's whole process group and `kill -KILL -1` every process the user
//! owns, so a line that is not a plain process id must be dropped before
//! anything is signalled. The parsing is the one place that can be checked
//! without killing anything.

mod support;

use support::recorded_processes::killable_process_ids;

#[test]
fn ordinary_process_ids_are_kept_in_order() {
    // Ids as a script's `echo $$ >> file` writes them, one per line, with the
    // trailing newline of the last.
    assert_eq!(killable_process_ids("4242\n77\n2\n"), vec![4242, 77, 2]);
}

#[test]
fn ids_that_would_signal_more_than_one_process_are_skipped() {
    // `0` is the caller's process group, `1` is `init`, and `-1` is every
    // process the user may signal. Ordinary ids around them are still kept,
    // so the guard is not simply refusing the whole file.
    assert_eq!(killable_process_ids("0\n1\n-1\n-4242\n99\n"), vec![99]);
}

#[test]
fn lines_that_are_not_whole_numbers_are_skipped() {
    // Empty lines, words, signs, fractions and a number too large for a
    // process id are all damage, not ids.
    let recorded = "\nabc\n+5\n12abc\n1.5\n99999999999999999999\n 300 \n";
    assert_eq!(killable_process_ids(recorded), vec![300]);
}
