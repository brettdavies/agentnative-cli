package main

import (
	"os"

	"github.com/urfave/cli/v2"
)

func main() {
	app := &cli.App{Name: "probe", Usage: "urfave/cli v2 probe", UseShortOptionHandling: true,
		Flags: []cli.Flag{
			&cli.BoolFlag{Name: "force", Aliases: []string{"f"}, Usage: "skip confirmation prompts"},
			&cli.IntFlag{Name: "limit", Aliases: []string{"n"}, Usage: "maximum number of results to return, a deliberately long description"},
			&cli.BoolFlag{Name: "dry-run", Usage: "print what would change"},
			&cli.StringFlag{Name: "output", Aliases: []string{"o"}, Usage: "write output to `FILE`"},
			&cli.BoolFlag{Name: "print-config-dir", Aliases: []string{"cd"}, Usage: "multi-letter alias"},
		}}
	app.Run(os.Args)
}
