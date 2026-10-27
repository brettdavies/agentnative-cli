//! Help text as a terminal shows it, so the classifier and the tokenizer read
//! columns and names from what a reader sees.

const TAB_STOP: usize = 8;
const ESCAPE: char = '\u{1b}';
const BACKSPACE: char = '\u{8}';
const BELL: char = '\u{7}';

/// `raw`, line for line, with ANSI escape sequences removed, each overstruck
/// character reduced to the one printed last (groff bold is `X\bX`), TABs
/// expanded to the next stop of eight, and each vertical edge of a box table
/// turned into a gap. A box-table row that leads with a `*` cell, where typer
/// and rich-click mark a required option, has the marker blanked, so the row
/// leads with its name. Lines keep their order and count.
pub(super) fn normalize(raw: &str) -> String {
    raw.lines().map(shown).collect::<Vec<_>>().join("\n")
}

fn shown(line: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(line.len());
    let mut boxed = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ESCAPE => skip_escape(&mut chars),
            BACKSPACE => {
                out.pop();
            }
            '\t' => {
                let stop = (out.len() / TAB_STOP + 1) * TAB_STOP;
                out.resize(stop, ' ');
            }
            '│' | '┃' | '║' => {
                boxed = true;
                out.extend([' ', ' ']);
            }
            _ => out.push(c),
        }
    }
    if boxed {
        blank_required_marker(&mut out);
    }
    out.into_iter().collect()
}

/// Blank a `*` that leads the row and stands in a cell of its own.
fn blank_required_marker(row: &mut [char]) {
    let Some(first) = row.iter().position(|c| !c.is_whitespace()) else {
        return;
    };
    let alone = row.get(first + 1..first + 3) == Some(&[' ', ' ']);
    if row[first] == '*' && alone {
        row[first] = ' ';
    }
}

/// Consume the rest of an escape sequence: a CSI sequence through its final
/// byte, an OSC sequence through its terminator, or one character.
fn skip_escape(chars: &mut std::str::Chars<'_>) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
        Some(']') => {
            let mut previous = ' ';
            for c in chars.by_ref() {
                if c == BELL || (previous == ESCAPE && c == '\\') {
                    break;
                }
                previous = c;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_sequences_are_removed() {
        assert_eq!(
            normalize("\u{1b}[1m--dates\u{1b}[0m  Show dates"),
            "--dates  Show dates"
        );
        assert_eq!(
            normalize("\u{1b}]8;;https://example.com\u{7}--link\u{1b}]8;;\u{7}  A link"),
            "--link  A link"
        );
        assert_eq!(
            normalize("\u{1b}]8;;https://example.com\u{1b}\\--link"),
            "--link"
        );
    }

    #[test]
    fn overstruck_text_reads_as_printed() {
        assert_eq!(normalize("-\u{8}--\u{8}-r\u{8}re\u{8}ec\u{8}c"), "--rec");
        assert_eq!(normalize("_\u{8}f_\u{8}i_\u{8}l_\u{8}e"), "file");
    }

    #[test]
    fn a_tab_runs_to_the_next_stop_of_eight() {
        assert_eq!(normalize("\t-f"), "        -f");
        assert_eq!(normalize("  -f\tforce"), "  -f    force");
        assert_eq!(normalize("    \tdesc"), "        desc");
    }

    #[test]
    fn a_box_edge_becomes_a_gap() {
        assert_eq!(
            normalize("│-d│--dates│Show dates│"),
            "  -d  --dates  Show dates  "
        );
    }

    #[test]
    fn a_required_marker_in_a_box_table_becomes_a_space() {
        assert_eq!(
            normalize("│ *  --target  -t  <str>  where to deploy [required] │"),
            "      --target  -t  <str>  where to deploy [required]   "
        );
        assert_eq!(
            normalize("  * --all: every item, as a bullet"),
            "  * --all: every item, as a bullet"
        );
        assert_eq!(
            normalize("│ *.md  files are read │"),
            "   *.md  files are read   "
        );
    }

    #[test]
    fn lines_keep_their_count_and_multibyte_text() {
        let raw = "选项:\n\n  -H, --header <HEADER>     自定义请求头\n";
        assert_eq!(normalize(raw).lines().count(), raw.lines().count());
        assert!(normalize(raw).contains("自定义请求头"));
    }
}
