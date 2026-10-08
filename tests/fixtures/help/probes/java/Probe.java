import picocli.CommandLine;
import picocli.CommandLine.*;
@Command(name = "probe", mixinStandardHelpOptions = true, version = "1", description = "picocli probe")
public class Probe implements Runnable {
  @Option(names = {"-f", "--force"}, description = "skip confirmation prompts") boolean force;
  @Option(names = {"-n", "--limit"}, paramLabel = "N", description = "maximum number of results to return, a deliberately long description so picocli has to wrap it onto continuation lines") int limit;
  @Option(names = "--dry-run", description = "print what would change") boolean dryRun;
  @Option(names = {"-cp", "--classpath"}, description = "multi-letter single-dash name") String cp;
  @Option(names = "-version-json", description = "single-dash word option") boolean vj;
  @Option(names = {"-c"}, description = "single c") boolean c;
  @Option(names = {"-p"}, description = "single p") boolean p;
  @Option(names = {"-o", "--output", "/out"}, description = "three names") String out;
  @Parameters(paramLabel = "PATH", arity = "0..1", description = "positional") String path;
  public void run() { System.out.println("cp=" + cp + " c=" + c + " p=" + p + " force=" + force); }
  public static void main(String[] a) { System.exit(new CommandLine(new Probe()).execute(a)); }
}
