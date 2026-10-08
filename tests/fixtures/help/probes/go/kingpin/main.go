package main

import (
	"os"

	"github.com/alecthomas/kingpin/v2"
)

func main() {
	app := kingpin.New("probe", "kingpin probe")
	app.Flag("force", "skip confirmation prompts").Short('f').Bool()
	app.Flag("limit", "maximum number of results to return, a deliberately long description so kingpin wraps it").Short('n').Int()
	app.Flag("dry-run", "print what would change").Bool()
	app.Flag("output", "write output here").Short('o').PlaceHolder("FILE").String()
	app.Flag("color", "colorize").Default("true").Bool()
	app.Parse(os.Args[1:])
}
