//! Stopping a conversion early, with a message, when this machine cannot finish it.
//!
//! Once a design is read the memory it holds is measured, not estimated. Mapping and writing
//! it needs more on top (the work Dutchman does on a decoded model keeps further copies of
//! it), so a machine with less free than a share of what the design already holds stops here
//! instead of running for hours and then running out. The message names the largest parts by
//! their `dutchman diagnose` numbers and their share of the memory.

use dutchman_core::{parts, Model};

/// What mapping and writing a decoded model needs on top of what it holds, as a share of it.
/// Measured on large Fusion assemblies, where the extra work is more than the model itself;
/// the lower figure here keeps a conversion that would only just fit from being stopped.
pub const EXTRA_SHARE: f64 = 0.75;

/// A design holding less than this is never stopped: its extra work cannot matter.
pub const MIN_HELD: u64 = 1 << 30;

/// How many of the largest parts the message names.
const NAMED: usize = 3;

/// The message to stop with, when `held` bytes (what this process holds with the design
/// read) and `available` bytes (still free) say the conversion cannot finish.
pub fn check(model: &Model, held: Option<u64>, available: Option<u64>) -> Option<String> {
    let (held, available) = (held?, available?);
    stop_message(held, available, &parts::catalog(model))
}

/// [`check`] over a catalog of parts.
pub fn stop_message(held: u64, available: u64, catalog: &[parts::PartInfo]) -> Option<String> {
    if held < MIN_HELD {
        return None;
    }
    let extra = (held as f64 * EXTRA_SHARE) as u64;
    if extra <= available {
        return None;
    }
    let gb = |bytes: u64| bytes as f64 / 1_000_000_000.0;
    let mut text = format!(
        "this conversion needs about {:.1} GB more memory and {:.1} GB are free (the design \
         already takes {:.1} GB)",
        gb(extra),
        gb(available),
        gb(held)
    );
    let total: u64 = catalog
        .iter()
        .filter(|part| !part.assembly)
        .map(|part| part.entities)
        .sum();
    let mut largest: Vec<_> = catalog
        .iter()
        .filter(|part| !part.assembly && part.entities > 0)
        .collect();
    largest.sort_by(|a, b| b.entities.cmp(&a.entities).then(a.number.cmp(&b.number)));
    largest.truncate(NAMED);
    if total > 0 && !largest.is_empty() {
        let named: Vec<String> = largest
            .iter()
            .map(|part| {
                let share = (held as f64 * part.entities as f64 / total as f64) as u64;
                format!("{} (about {:.1} GB)", part.number, gb(share))
            })
            .collect();
        text.push_str(&format!(
            "; part{} {} {} the largest",
            if named.len() == 1 { "" } else { "s" },
            join(&named),
            if named.len() == 1 { "is" } else { "are" }
        ));
    }
    text.push_str(
        ". Run `dutchman diagnose` on the file to see which parts fail, run slowly or take the \
         most memory, or convert one part with --part N (--no-memory-check converts anyway)",
    );
    Some(text)
}

fn join(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, last] => format!("{first} and {last}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn part(number: usize, entities: u64, assembly: bool) -> parts::PartInfo {
        parts::PartInfo {
            number,
            parent: None,
            depth: 0,
            assembly,
            bodies: 1,
            entities,
            subtree_entities: entities,
            name: format!("secret name {number}"),
        }
    }

    const GB: u64 = 1_000_000_000;

    #[test]
    fn a_machine_with_room_is_not_stopped() {
        let catalog = [part(1, 10, false)];
        assert_eq!(stop_message(8 * GB, 20 * GB, &catalog), None);
        // A small design is never stopped, however little is free.
        assert_eq!(stop_message(GB / 2, 0, &catalog), None);
    }

    #[test]
    fn the_message_names_the_largest_parts_by_number_and_never_by_name() {
        let catalog = [
            part(1, 100, false),
            part(2, 5, true),
            part(3, 300, false),
            part(4, 50, false),
            part(17, 550, false),
        ];
        let text = stop_message(10 * GB, 2 * GB, &catalog).unwrap();
        assert!(
            text.starts_with("this conversion needs about 7.5 GB more memory and 2.0 GB are free")
        );
        assert!(
            text.contains(
                "parts 17 (about 5.5 GB), 3 (about 3.0 GB), and 1 (about 1.0 GB) are the largest"
            ),
            "{text}"
        );
        assert!(text.contains("dutchman diagnose"), "{text}");
        assert!(text.contains("--part N"), "{text}");
        assert!(!text.contains("secret"), "{text}");
    }

    #[test]
    fn two_parts_read_as_a_pair() {
        let catalog = [part(4, 60, false), part(17, 90, false)];
        let text = stop_message(10 * GB, 0, &catalog).unwrap();
        assert!(
            text.contains("parts 17 (about 6.0 GB) and 4 (about 4.0 GB) are the largest"),
            "{text}"
        );
    }
}
