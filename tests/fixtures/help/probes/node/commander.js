const { Command } = require('commander');
const p = new Command('probe').description('commander probe');
p.option('-f, --force', 'skip confirmation prompts')
 .option('-n, --limit <n>', 'maximum number of results to return, a deliberately long description so commander has to wrap it onto continuation lines')
 .option('--dry-run', 'print what would change')
 .option('-v, --verbose', 'increase verbosity')
 .option('-o, --output [file]', 'write output here (optional value)')
 .option('--no-color', 'disable color');
p.command('sub').description('a subcommand');
p.parse();
