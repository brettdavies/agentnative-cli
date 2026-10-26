const yargs = require('yargs/yargs');
const { hideBin } = require('yargs/helpers');
yargs(hideBin(process.argv)).scriptName('probe').usage('$0 [options]')
 .option('force', { alias: 'f', type: 'boolean', describe: 'skip confirmation prompts' })
 .option('limit', { alias: 'n', type: 'number', describe: 'maximum number of results to return, a deliberately long description so yargs has to wrap it onto continuation lines' })
 .option('dry-run', { type: 'boolean', describe: 'print what would change' })
 .option('output', { alias: ['o', 'out'], type: 'string', describe: 'write output here', demandOption: false })
 .option('print-config-dir', { alias: 'cd', type: 'boolean', describe: 'multi-letter alias' })
 .command('sub', 'a subcommand').help().argv;
