//! Parse-only audit target for `anc audit --source`; never compiled.
//!
//! Each loop body carries a multi-byte glyph run that straddles byte 80 of
//! the collapsed loop text, and the file contains no clamping indicator, so
//! every loop reaches the evidence preview formatter.

fn main() {
    let mut frames_seen = Vec::new();
    for frame in ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"].iter() {
        frames_seen.push(*frame);
    }

    let names = ["太郎".to_string(), "花子".to_string()];
    for name in names.iter() {
        eprintln!("名前は{name}です。よろしくお願いします。");
    }

    let winners = ["a".to_string(), "b".to_string()];
    for winner in winners.iter() {
        eprintln!("congratulations {winner} 🏆🏆🏆🏆🏆🏆🏆🏆🏆🏆🏆🏆");
    }

    eprintln!("{}", frames_seen.len());
}
