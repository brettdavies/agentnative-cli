import click
@click.command()
@click.option("-f", "--force", is_flag=True, help="skip confirmation prompts")
@click.option("-n", "--limit", type=int, help="maximum number of results to return, a deliberately long description so that click has to wrap it onto continuation lines")
@click.option("--dry-run", is_flag=True, help="print what would change")
@click.option("-v", "--verbose", count=True, help="increase verbosity")
@click.option("-o", "--output", type=click.Path(), help="write output here")
@click.option("-cd", "--print-config-dir", is_flag=True, help="multi-letter single-dash alias")
@click.option("-no-color", "no_color", is_flag=True, help="single-dash word option")
@click.option("--color/--no-color2", default=True, help="boolean pair")
@click.argument("path", required=False)
def main(**kw):
    """click probe. Mentions --force in prose and
    --dry-run at the start of a wrapped line."""
main()
