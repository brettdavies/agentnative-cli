require "thor"
class Probe < Thor
  def self.exit_on_failure? = true
  desc "deploy TARGET", "deploy a target"
  long_desc "Deploys TARGET. Mentions --force in prose."
  method_option :force, aliases: "-f", type: :boolean, desc: "skip confirmation prompts"
  method_option :limit, aliases: "-n", type: :numeric, desc: "maximum number of results to return, a deliberately long description so Thor has to wrap it onto continuation lines"
  method_option :dry_run, type: :boolean, desc: "print what would change"
  method_option :output, aliases: ["-o", "--out"], type: :string, banner: "FILE", desc: "write output here"
  method_option :print_config_dir, aliases: "-cd", type: :boolean, desc: "multi-letter alias"
  method_option :c, type: :boolean, desc: "single c"
  method_option :d, type: :boolean, desc: "single d"
  def deploy(target) = puts(options.to_h.inspect)
end
Probe.start(ARGV)
