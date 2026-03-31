"""
CLI command handlers for the Kryos package manager.

These functions are designed to be registered as subcommands in cli.py.
Each accepts an argparse.Namespace and handles user-facing output.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


# ---------------------------------------------------------------------------
# ANSI helpers (mirror cli.py conventions)
# ---------------------------------------------------------------------------

_USE_COLOR = hasattr(sys.stdout, "isatty") and sys.stdout.isatty()


def _c(code: str, text: str) -> str:
    return f"\033[{code}m{text}\033[0m" if _USE_COLOR else text


def _red(t: str) -> str: return _c("31", t)
def _green(t: str) -> str: return _c("32", t)
def _yellow(t: str) -> str: return _c("33", t)
def _cyan(t: str) -> str: return _c("36", t)
def _bold(t: str) -> str: return _c("1", t)
def _dim(t: str) -> str: return _c("2", t)


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

def cmd_init(args: argparse.Namespace) -> None:
    """kryos init — create a new Kryos project."""
    from kryos.compiler.packages import init_project

    path = getattr(args, 'path', None) or '.'
    name = getattr(args, 'name', None)

    msg = init_project(path=path, name=name)
    print(_green(msg))


def cmd_add(args: argparse.Namespace) -> None:
    """kryos add <package> [--version VER] [--category CAT] — add a dependency."""
    from kryos.compiler.packages import add_dependency

    package = args.package
    version = getattr(args, 'version', '*') or '*'
    category = getattr(args, 'category', 'kryos') or 'kryos'

    # Support shorthand: "python/numpy" -> category=python, name=numpy
    if '/' in package:
        cat, pkg = package.split('/', 1)
        if cat in ('python', 'rust', 'c'):
            category = cat
            package = pkg

    try:
        msg = add_dependency(package, version, category)
        print(_green(msg))
    except FileNotFoundError as e:
        print(_red(str(e)))
        sys.exit(1)


def cmd_remove(args: argparse.Namespace) -> None:
    """kryos remove <package> — remove a dependency."""
    from kryos.compiler.packages import remove_dependency

    package = args.package
    category = getattr(args, 'category', 'kryos') or 'kryos'

    if '/' in package:
        cat, pkg = package.split('/', 1)
        if cat in ('python', 'rust', 'c'):
            category = cat
            package = pkg

    try:
        msg = remove_dependency(package, category)
        print(_green(msg) if 'Removed' in msg else _yellow(msg))
    except FileNotFoundError as e:
        print(_red(str(e)))
        sys.exit(1)


def cmd_deps(args: argparse.Namespace) -> None:
    """kryos deps — list all dependencies."""
    from kryos.compiler.packages import list_dependencies

    try:
        deps = list_dependencies()
    except FileNotFoundError as e:
        print(_red(str(e)))
        sys.exit(1)

    has_any = False
    for category, packages in deps.items():
        if not packages:
            continue
        has_any = True
        print(_bold(f"[{category}]"))
        for name, version in packages.items():
            print(f"  {_cyan(name)} = \"{version}\"")
        print()

    if not has_any:
        print(_dim("No dependencies declared."))


def cmd_install(args: argparse.Namespace) -> None:
    """kryos install — resolve and install dependencies."""
    from kryos.compiler.packages import install_dependencies

    try:
        messages = install_dependencies()
    except FileNotFoundError as e:
        print(_red(str(e)))
        sys.exit(1)

    if not messages:
        print(_dim("No dependencies to install."))
        return

    print(_bold("Resolving dependencies...\n"))
    for msg in messages:
        if '[MISSING]' in msg:
            print(_yellow(msg))
        elif '[OK]' in msg:
            print(_green(msg))
        else:
            print(_dim(msg))
    print()
    print(_green("Done."))


def cmd_publish(args: argparse.Namespace) -> None:
    """kryos publish — publish package to local registry."""
    from kryos.compiler.packages import publish_local

    try:
        msg = publish_local()
        print(_green(msg))
    except FileNotFoundError as e:
        print(_red(str(e)))
        sys.exit(1)


# ---------------------------------------------------------------------------
# Registration helper — called from cli.py to wire up subparsers
# ---------------------------------------------------------------------------

def register_package_commands(subparsers: argparse._SubParsersAction) -> dict:
    """Register all package-management subcommands.

    Returns a dict mapping command name -> handler function.
    """
    # init
    init_p = subparsers.add_parser("init", help="Create a new Kryos project")
    init_p.add_argument("path", nargs="?", default=".", help="Project directory (default: .)")
    init_p.add_argument("--name", help="Project name (default: directory name)")

    # add
    add_p = subparsers.add_parser("add", help="Add a dependency")
    add_p.add_argument("package", help="Package name (e.g. kryos_ml or python/numpy)")
    add_p.add_argument("--version", "-v", default="*", help="Version constraint (default: *)")
    add_p.add_argument("--category", "-c", default="kryos",
                       choices=["kryos", "python", "rust", "c"],
                       help="Dependency category (default: kryos)")

    # remove
    rm_p = subparsers.add_parser("remove", help="Remove a dependency")
    rm_p.add_argument("package", help="Package name (e.g. kryos_ml or python/numpy)")
    rm_p.add_argument("--category", "-c", default="kryos",
                      choices=["kryos", "python", "rust", "c"],
                      help="Dependency category (default: kryos)")

    # deps
    subparsers.add_parser("deps", help="List all dependencies")

    # install
    subparsers.add_parser("install", help="Install dependencies")

    # publish
    subparsers.add_parser("publish", help="Publish package to local registry")

    return {
        "init": cmd_init,
        "add": cmd_add,
        "remove": cmd_remove,
        "deps": cmd_deps,
        "install": cmd_install,
        "publish": cmd_publish,
    }
