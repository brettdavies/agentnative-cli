//! Shared applicability gate for list-style CLI audits.
//!
//! `p7-should-limit` and `p7-may-cursor-pagination` only apply when the
//! target CLI has list-style subcommands. A behavioral-layer heuristic looks
//! for common list verbs in the top-level subcommand surface.
//!
//! Verbs that always count: `list`, `ls`, `search`, `query`, `find`.
//! `show` and `get` also name commands that print or fetch one thing
//! (`terraform show` prints one state, `terraform get` downloads modules,
//! `helm get` reads one release), so they count only when their summary in
//! the command list describes returning several results: it leads with an
//! output verb (`show`, `display`, `list`, ...) and says "list", "all", or
//! "many", or names a plural noun (`Display one or many resources`). A CLI
//! without any counted subcommand vacuously skips the audit.

use crate::runner::HelpOutput;

const LIST_VERBS: &[&str] = &["list", "ls", "search", "query", "find"];

/// Verbs that count only when the summary describes several results.
const SINGLE_OR_LIST_VERBS: &[&str] = &["show", "get"];

/// Leading summary verbs that describe output, matched with or without a
/// third-person `s` (`Shows`, `Lists`).
const OUTPUT_VERBS: &[&str] = &[
    "show", "display", "print", "list", "get", "return", "retrieve", "output", "view",
];

/// Summary words that describe several results on their own.
const SEVERAL_WORDS: &[&str] = &["list", "lists", "all", "many"];

/// The top-level subcommands that count as list operations, in help order.
/// Empty when the gate does not fire.
pub(crate) fn list_style_subcommands(help: &HelpOutput) -> Vec<&str> {
    help.subcommands()
        .iter()
        .map(String::as_str)
        .filter(|name| {
            LIST_VERBS
                .iter()
                .any(|verb| name.eq_ignore_ascii_case(verb))
                || (SINGLE_OR_LIST_VERBS
                    .iter()
                    .any(|verb| name.eq_ignore_ascii_case(verb))
                    && help
                        .subcommand_summary(name)
                        .is_some_and(describes_several_results))
        })
        .collect()
}

fn describes_several_results(summary: &str) -> bool {
    let lower = summary.to_lowercase();
    let mut words = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty());
    let Some(lead) = words.next() else {
        return false;
    };
    let lead = lead
        .strip_suffix('s')
        .filter(|v| OUTPUT_VERBS.contains(v))
        .unwrap_or(lead);
    if !OUTPUT_VERBS.contains(&lead) {
        return false;
    }
    words.any(|w| SEVERAL_WORDS.contains(&w) || is_plural_noun(w))
}

/// A word that reads as an English plural: four or more letters ending in
/// `s`, excluding the `-ss`, `-us`, and `-is` endings of singular words
/// (`process`, `status`, `analysis`).
fn is_plural_noun(word: &str) -> bool {
    word.len() >= 4
        && word.ends_with('s')
        && !["ss", "us", "is"].iter().any(|end| word.ends_with(end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_list_verb() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list    List items.\n  build   Build.\n",
        );
        assert!(!list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn detects_search_verb() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  search    Search items.\n");
        assert!(!list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn case_insensitive_match() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  LIST    UPPERCASE.\n");
        assert!(!list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn rejects_non_list_verbs() {
        // Sanity check against the verb list — `audit`, `build`, `run` are
        // common non-list verbs that should fail the gate.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits.\n  build    Build.\n  run    Run.\n",
        );
        assert!(list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn empty_subcommand_list_returns_false() {
        let help = HelpOutput::from_raw("Usage: tool [OPTIONS]\n\nOptions:\n  -h, --help\n");
        assert!(list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn show_and_get_count_only_when_their_summary_describes_several_results() {
        // Each summary as the named tool's command list prints it.
        let cases = [
            ("get", "Install or upgrade remote Terraform modules", false), // terraform 1.16
            ("show", "Show the current state or a saved plan", false),     // terraform 1.16
            (
                "get",
                "download extended information of a named release",
                false,
            ), // helm 4.3
            ("show", "show information of a chart", false),                // helm 4.3
            ("show", "Show information for a model", false),               // ollama 0.35
            ("show", "Show revision metadata and diff", false),            // jj
            ("get", "Display one or many resources", true),                // kubectl 1.37
            ("show", "Show various types of objects", true),               // git
        ];
        let verdicts: Vec<(&str, &str, bool)> = cases
            .iter()
            .map(|&(name, summary, _)| {
                let help = HelpOutput::from_raw(format!(
                    "Usage: tool <COMMAND>\n\nCommands:\n  {name}  {summary}\n"
                ));
                (name, summary, !list_style_subcommands(&help).is_empty())
            })
            .collect();
        assert_eq!(verdicts, cases);
    }

    #[test]
    fn terraform_counts_query_but_not_get_or_show() {
        // Excerpt of terraform 1.16's `terraform --help`.
        let help = HelpOutput::from_raw(
            "\
Usage: terraform [global options] <subcommand> [args]

All other commands:
  console       Try Terraform expressions at an interactive command prompt
  get           Install or upgrade remote Terraform modules
  modules       Show all declared modules in a working directory
  query         Search and list remote infrastructure with Terraform
  show          Show the current state or a saved plan
  version       Show the current Terraform version
",
        );
        assert_eq!(list_style_subcommands(&help), ["query"]);
    }

    #[test]
    fn qmd_get_without_a_described_result_set_is_not_counted() {
        // Excerpt of qmd's `qmd --help`. Its `get` entry has no two-space
        // gap before the summary, so the command list gives it none.
        let help = HelpOutput::from_raw(
            "\
qmd — Quick Markdown Search

Usage:
  qmd <command> [options]

Primary commands:
  qmd query <query>             - Hybrid search with auto expansion + reranking (recommended)
  qmd search <query>            - Full-text BM25 keywords (no LLM)
  qmd get <file>[:from[:count]] - Show a document (line-numbered; #docid in header)
  qmd mcp                       - Start the MCP server (stdio transport for AI agents)
",
        );
        assert_eq!(list_style_subcommands(&help), ["query", "search"]);
    }
}
