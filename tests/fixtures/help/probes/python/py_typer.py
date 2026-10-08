import typer
from typing import Optional
app = typer.Typer(add_completion=False)
@app.command()
def main(force: bool = typer.Option(False, "-f", "--force", help="skip confirmation prompts"),
         limit: Optional[int] = typer.Option(None, "-n", "--limit", help="maximum number of results to return, a deliberately long description that wraps"),
         dry_run: bool = typer.Option(False, "--dry-run", help="print what would change"),
         cd: bool = typer.Option(False, "-cd", "--print-config-dir", help="multi-letter single-dash alias"),
         output: Optional[str] = typer.Option(None, "-o", "--output", help="write output here")):
    """typer probe"""
app()
