import rich_click as click
@click.command()
@click.option("-f", "--force", is_flag=True, help="skip confirmation prompts")
@click.option("-n", "--limit", type=int, help="maximum number of results to return, a deliberately long description that wraps")
@click.option("--dry-run", is_flag=True, help="print what would change")
@click.option("-cd", "--print-config-dir", is_flag=True, help="multi-letter single-dash alias")
def main(**kw):
    """rich-click probe"""
main()
