import {Command, Flags, Args} from '@oclif/core'
class Deploy extends Command {
  static description = 'oclif probe'
  static args = {target: Args.string({description: 'deploy target'})}
  static flags = {
    force: Flags.boolean({char: 'f', description: 'skip confirmation prompts'}),
    limit: Flags.integer({char: 'n', description: 'maximum number of results to return, a deliberately long description so oclif has to wrap it onto continuation lines'}),
    'dry-run': Flags.boolean({description: 'print what would change'}),
    output: Flags.string({char: 'o', description: 'write output here', aliases: ['out'], charAliases: ['O']}),
    color: Flags.boolean({description: 'colorize', allowNo: true}),
  }
  async run() { const {flags} = await this.parse(Deploy); console.log(JSON.stringify(flags)) }
}
export const COMMANDS = {deploy: Deploy}
