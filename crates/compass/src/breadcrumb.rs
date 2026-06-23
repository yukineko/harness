//! breadcrumb (DESIGN §5⑤ / §14) — the Stop-hook step that writes "the next
//! physical action" into the charter so the ②"blank after a checkpoint" pain
//! doesn't recur. No LLM, never blocks: it ALWAYS exits 0.
//!
//! It mirrors consulting-agent's `--hook-stop` block-parse approach: the
//! assistant signals an explicit next step by emitting a fenced block
//!
//! ````markdown
//! ```compass-next
//! <the next physical action>
//! ```
//! ````
//!
//! in its final message. If present, that text is written to
//! `charter.next_action`. If absent we do nothing — we never GUESS a next action
//! from free prose (a wrong breadcrumb is worse than none).

use std::path::Path;

use crate::charter::Charter;

/// The fence tag the assistant uses to mark its next physical action.
const FENCE_TAG: &str = "compass-next";

/// Extract the `compass-next` block body from an assistant message, if present.
///
/// Looks for a fenced block opening with ```` ```compass-next ```` (optionally
/// with trailing whitespace) and closes at the next line that is exactly a fence
/// (```` ``` ````). The body is the lines between, trimmed. Returns `None` when
/// no such block exists or the body is empty. If several blocks appear, the
/// LAST one wins (the assistant's most recent intent).
pub fn extract_next_action(message: &str) -> Option<String> {
    let lines: Vec<&str> = message.lines().collect();
    let mut found: Option<String> = None;

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        // Opening fence: ```compass-next (allow ` ```compass-next ` with spaces).
        let is_open = trimmed
            .strip_prefix("```")
            .map(|rest| rest.trim() == FENCE_TAG)
            .unwrap_or(false);
        if !is_open {
            i += 1;
            continue;
        }
        // Collect body lines until the closing fence.
        let mut body: Vec<&str> = Vec::new();
        let mut j = i + 1;
        let mut closed = false;
        while j < lines.len() {
            if lines[j].trim() == "```" {
                closed = true;
                break;
            }
            body.push(lines[j]);
            j += 1;
        }
        if closed {
            let text = body.join("\n").trim().to_string();
            if !text.is_empty() {
                found = Some(text); // last block wins
            }
            i = j + 1;
        } else {
            // Unterminated fence: stop scanning (don't misread the rest).
            break;
        }
    }

    found
}

/// Write `next_action` into the charter at `charter_path` (load → set → save).
/// Best-effort: returns Ok even if nothing changed; errors only bubble up I/O
/// problems, which the caller swallows (the hook must never break a turn).
pub fn write_next_action(charter_path: &Path, next_action: &str) -> anyhow::Result<()> {
    // Load the existing charter; if it doesn't parse/exist yet, start blank so a
    // breadcrumb can still seed `next_action` (the charter may be carved later).
    let mut charter = Charter::load(charter_path).unwrap_or_default();
    charter.next_action = next_action.trim().to_string();
    charter.save(charter_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_compass_next_block() {
        let msg = "Done for now.\n\n```compass-next\nWire route_command to read --file.\n```\n\nThanks!";
        assert_eq!(
            extract_next_action(msg).as_deref(),
            Some("Wire route_command to read --file.")
        );
    }

    #[test]
    fn multiline_block_body_is_joined_and_trimmed() {
        let msg = "```compass-next\n  first line\n  second line  \n```";
        assert_eq!(
            extract_next_action(msg).as_deref(),
            Some("first line\n  second line")
        );
    }

    #[test]
    fn no_block_yields_none() {
        let msg = "I think the next step is probably to wire the route command, but let's see.";
        assert_eq!(extract_next_action(msg), None);
    }

    #[test]
    fn empty_block_yields_none() {
        let msg = "```compass-next\n\n```";
        assert_eq!(extract_next_action(msg), None);
    }

    #[test]
    fn unterminated_block_yields_none() {
        let msg = "```compass-next\nthis fence never closes";
        assert_eq!(extract_next_action(msg), None);
    }

    #[test]
    fn last_block_wins() {
        let msg = "```compass-next\nold plan\n```\n\nactually:\n\n```compass-next\nnew plan\n```";
        assert_eq!(extract_next_action(msg).as_deref(), Some("new plan"));
    }

    #[test]
    fn ignores_other_fenced_blocks() {
        let msg = "```rust\nfn main() {}\n```\n\n```compass-next\nrun the tests\n```";
        assert_eq!(extract_next_action(msg).as_deref(), Some("run the tests"));
    }

    #[test]
    fn write_next_action_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".compass").join("charter.md");
        let charter = Charter {
            north_star: "Ship compass.".to_string(),
            definition_of_done: vec!["crate builds".to_string()],
            ..Charter::default()
        };
        charter.save(&path).unwrap();

        write_next_action(&path, "  Wire the skill.  ").expect("write");
        let reloaded = Charter::load(&path).expect("load");
        assert_eq!(reloaded.next_action, "Wire the skill.");
        // Existing fields preserved.
        assert_eq!(reloaded.north_star, "Ship compass.");
        assert_eq!(reloaded.definition_of_done, vec!["crate builds"]);
    }
}
