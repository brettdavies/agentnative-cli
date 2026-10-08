package main

import "github.com/alecthomas/kong"

var CLI struct {
	Force   bool   `short:"f" help:"skip confirmation prompts"`
	Limit   int    `short:"n" help:"maximum number of results to return, a deliberately long description so kong wraps it"`
	DryRun  bool   `help:"print what would change"`
	Output  string `short:"o" placeholder:"FILE" help:"write output here"`
	Color   bool   `negatable:"" default:"true" help:"colorize"`
	Verbose int    `short:"v" type:"counter" help:"increase verbosity"`
}

func main() { kong.Parse(&CLI) }
