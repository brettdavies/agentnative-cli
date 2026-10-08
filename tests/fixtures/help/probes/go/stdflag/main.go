package main

import "flag"

func main() {
	flag.Bool("f", false, "skip confirmation prompts")
	flag.Bool("force", false, "skip confirmation prompts (long spelling)")
	flag.Int("n", 0, "maximum `count` of results")
	flag.Bool("no-color", false, "disable color")
	flag.Bool("auto-approve", false, "skip interactive approval, a deliberately long description\nthat the author wrapped by hand")
	flag.String("chdir", "", "switch to `DIR` before running")
	flag.Bool("v", false, "verbose")
	flag.Parse()
}
