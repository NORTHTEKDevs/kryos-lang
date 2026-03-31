"""
Kryos Package Manager
=====================
Bootstrap implementation of package management for the Kryos language.

Handles:
- kryos.toml project configuration (minimal TOML parser included)
- Local package registry (~/.kryos/packages/)
- Semver version resolution
- CLI-callable functions for init, add, remove, list, resolve, install
- Module resolution for `use` imports against installed packages
"""

from __future__ import annotations

import os
import re
import shutil
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


# ===========================================================================
# Minimal TOML parser (read/write, stdlib only)
# ===========================================================================

class TomlError(Exception):
    """Error parsing or writing TOML."""
    pass


def _strip_comment(line: str) -> str:
    """Strip inline comments (not inside strings)."""
    in_str = False
    for i, ch in enumerate(line):
        if ch == '"' and (i == 0 or line[i - 1] != '\\'):
            in_str = not in_str
        elif ch == '#' and not in_str:
            return line[:i].rstrip()
    return line


def _parse_value(raw: str) -> Any:
    """Parse a TOML value string into a Python object."""
    raw = raw.strip()
    if not raw:
        raise TomlError("Empty value")

    # String
    if raw.startswith('"') and raw.endswith('"'):
        return raw[1:-1].replace('\\"', '"').replace('\\n', '\n').replace('\\t', '\t')

    # Boolean
    if raw == 'true':
        return True
    if raw == 'false':
        return False

    # Integer
    try:
        return int(raw)
    except ValueError:
        pass

    # Float
    try:
        return float(raw)
    except ValueError:
        pass

    # Array (single-line only for bootstrap)
    if raw.startswith('[') and raw.endswith(']'):
        inner = raw[1:-1].strip()
        if not inner:
            return []
        items = []
        current = ''
        depth = 0
        in_s = False
        for ch in inner:
            if ch == '"' and (not current or current[-1] != '\\'):
                in_s = not in_s
            if not in_s:
                if ch in '[{':
                    depth += 1
                elif ch in ']}':
                    depth -= 1
                elif ch == ',' and depth == 0:
                    items.append(_parse_value(current.strip()))
                    current = ''
                    continue
            current += ch
        if current.strip():
            items.append(_parse_value(current.strip()))
        return items

    raise TomlError(f"Cannot parse TOML value: {raw!r}")


def parse_toml(text: str) -> Dict[str, Any]:
    """Parse a TOML string into a nested dict.

    Supports:
    - key = value pairs
    - [section] and [section.subsection] headers
    - Strings, ints, floats, bools, arrays
    - Comments (#)
    """
    result: Dict[str, Any] = {}
    current_section = result

    # Track the dotted path so we can navigate into nested dicts
    section_path: List[str] = []

    for lineno, raw_line in enumerate(text.splitlines(), 1):
        line = _strip_comment(raw_line).strip()
        if not line:
            continue

        # Section header
        m = re.match(r'^\[([^\]]+)\]$', line)
        if m:
            section_path = [k.strip() for k in m.group(1).split('.')]
            current_section = result
            for key in section_path:
                if key not in current_section:
                    current_section[key] = {}
                current_section = current_section[key]
                if not isinstance(current_section, dict):
                    raise TomlError(f"Line {lineno}: section path conflicts with existing value")
            continue

        # Key = value
        eq_idx = line.find('=')
        if eq_idx == -1:
            raise TomlError(f"Line {lineno}: expected key = value, got: {line!r}")

        key = line[:eq_idx].strip()
        val_str = line[eq_idx + 1:].strip()

        try:
            value = _parse_value(val_str)
        except TomlError as e:
            raise TomlError(f"Line {lineno}: {e}") from None

        current_section[key] = value

    return result


def dump_toml(data: Dict[str, Any], indent: int = 0) -> str:
    """Serialize a nested dict back to TOML format."""
    lines: List[str] = []
    sections: List[Tuple[str, dict]] = []

    # First pass: write top-level key=value pairs
    for k, v in data.items():
        if isinstance(v, dict):
            sections.append((k, v))
        else:
            lines.append(f"{k} = {_format_value(v)}")

    # Second pass: write sections
    for section_name, section_dict in sections:
        _dump_section(lines, [section_name], section_dict)

    return '\n'.join(lines) + '\n'


def _dump_section(lines: List[str], path: List[str], data: Dict[str, Any]) -> None:
    """Recursively dump a TOML section."""
    # Collect plain values and sub-sections
    plain: List[Tuple[str, Any]] = []
    nested: List[Tuple[str, dict]] = []

    for k, v in data.items():
        if isinstance(v, dict):
            nested.append((k, v))
        else:
            plain.append((k, v))

    # Write section header + plain values if any exist
    if plain or not nested:
        lines.append('')
        lines.append(f"[{'.'.join(path)}]")
        for k, v in plain:
            lines.append(f"{k} = {_format_value(v)}")

    # Write nested sub-sections
    for sub_name, sub_dict in nested:
        _dump_section(lines, path + [sub_name], sub_dict)


def _format_value(v: Any) -> str:
    """Format a Python value as a TOML value string."""
    if isinstance(v, bool):
        return 'true' if v else 'false'
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        return str(v)
    if isinstance(v, str):
        escaped = v.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n')
        return f'"{escaped}"'
    if isinstance(v, list):
        items = ', '.join(_format_value(i) for i in v)
        return f'[{items}]'
    raise TomlError(f"Cannot serialize type {type(v).__name__} to TOML")


# ===========================================================================
# Semver utilities
# ===========================================================================

class SemVer:
    """Minimal semantic version with comparison operators."""

    __slots__ = ('major', 'minor', 'patch', 'raw')

    def __init__(self, version_str: str):
        self.raw = version_str.strip()
        parts = self.raw.split('.')
        self.major = int(parts[0]) if len(parts) > 0 else 0
        self.minor = int(parts[1]) if len(parts) > 1 else 0
        self.patch = int(parts[2]) if len(parts) > 2 else 0

    def _tuple(self) -> Tuple[int, int, int]:
        return (self.major, self.minor, self.patch)

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, SemVer):
            return NotImplemented
        return self._tuple() == other._tuple()

    def __lt__(self, other: SemVer) -> bool:
        return self._tuple() < other._tuple()

    def __le__(self, other: SemVer) -> bool:
        return self._tuple() <= other._tuple()

    def __gt__(self, other: SemVer) -> bool:
        return self._tuple() > other._tuple()

    def __ge__(self, other: SemVer) -> bool:
        return self._tuple() >= other._tuple()

    def __repr__(self) -> str:
        return f"SemVer({self.raw!r})"

    def __str__(self) -> str:
        return self.raw


def version_satisfies(version: str, constraint: str) -> bool:
    """Check if a version string satisfies a constraint.

    Supports:
      "1.0"         — exact match (major.minor)
      "1.0.3"       — exact match (major.minor.patch)
      ">=1.0"       — greater-or-equal
      "<=1.0"       — less-or-equal
      ">1.0"        — greater
      "<1.0"        — less
      "^1.0"        — compatible (same major, >= given)
      "~1.0"        — approximately (same major.minor, >= given)
      "*"           — any version
    """
    constraint = constraint.strip()
    if constraint == '*':
        return True

    ver = SemVer(version)

    if constraint.startswith('>='):
        return ver >= SemVer(constraint[2:])
    if constraint.startswith('<='):
        return ver <= SemVer(constraint[2:])
    if constraint.startswith('>') and not constraint.startswith('>='):
        return ver > SemVer(constraint[1:])
    if constraint.startswith('<') and not constraint.startswith('<='):
        return ver < SemVer(constraint[1:])
    if constraint.startswith('^'):
        base = SemVer(constraint[1:])
        return ver >= base and ver.major == base.major
    if constraint.startswith('~'):
        base = SemVer(constraint[1:])
        return ver >= base and ver.major == base.major and ver.minor == base.minor

    # Exact match
    base = SemVer(constraint)
    return ver.major == base.major and ver.minor == base.minor and (
        base.patch == 0 or ver.patch == base.patch
    )


# ===========================================================================
# Paths & constants
# ===========================================================================

KRYOS_HOME = Path.home() / '.kryos'
PACKAGES_DIR = KRYOS_HOME / 'packages'
CONFIG_FILE = 'kryos.toml'

DEFAULT_CONFIG = {
    'package': {
        'name': '',
        'version': '0.1.0',
        'description': '',
        'authors': [],
        'license': 'MIT',
        'kryos_version': '>=0.1.0',
    },
    'dependencies': {},
    'build': {
        'target': 'native',
        'optimization': 'release',
    },
    'capabilities': {
        'allowed': ['compute'],
    },
}


# ===========================================================================
# Config I/O
# ===========================================================================

def _config_path(project_dir: Optional[str] = None) -> Path:
    """Return the path to kryos.toml in the given or current directory."""
    base = Path(project_dir) if project_dir else Path.cwd()
    return base / CONFIG_FILE


def load_config(project_dir: Optional[str] = None) -> Dict[str, Any]:
    """Load and parse kryos.toml from the project directory."""
    p = _config_path(project_dir)
    if not p.exists():
        raise FileNotFoundError(f"No {CONFIG_FILE} found in {p.parent}")
    return parse_toml(p.read_text(encoding='utf-8'))


def save_config(config: Dict[str, Any], project_dir: Optional[str] = None) -> Path:
    """Write config dict back to kryos.toml."""
    p = _config_path(project_dir)
    p.write_text(dump_toml(config), encoding='utf-8')
    return p


# ===========================================================================
# CLI-callable functions
# ===========================================================================

def init_project(path: Optional[str] = None, name: Optional[str] = None) -> str:
    """Create a new kryos.toml in the given directory.

    Returns a status message.
    """
    project_dir = Path(path) if path else Path.cwd()
    project_dir.mkdir(parents=True, exist_ok=True)

    cfg_path = project_dir / CONFIG_FILE
    if cfg_path.exists():
        return f"Project already initialized ({cfg_path})"

    config = _deep_copy_dict(DEFAULT_CONFIG)
    config['package']['name'] = name or project_dir.name

    # Create src directory
    src_dir = project_dir / 'src'
    src_dir.mkdir(exist_ok=True)

    # Create a starter main.kry
    main_file = src_dir / 'main.kry'
    if not main_file.exists():
        main_file.write_text(
            '// Generated by kryos init\n'
            'fn main() {\n'
            '    print("Hello from Kryos!")\n'
            '}\n\n'
            'main()\n',
            encoding='utf-8',
        )

    save_config(config, str(project_dir))
    return f"Initialized Kryos project in {project_dir}"


def add_dependency(
    name: str,
    version: str = '*',
    category: str = 'kryos',
    project_dir: Optional[str] = None,
) -> str:
    """Add a dependency to kryos.toml.

    Args:
        name: Package name.
        version: Version constraint string.
        category: One of 'kryos', 'python', 'rust', 'c'.
        project_dir: Project root (defaults to cwd).

    Returns a status message.
    """
    config = load_config(project_dir)
    deps = config.setdefault('dependencies', {})

    if category == 'kryos':
        deps[name] = version
    else:
        sub = deps.setdefault(category, {})
        if isinstance(sub, dict):
            sub[name] = version
        else:
            raise TomlError(f"dependencies.{category} is not a table")

    save_config(config, project_dir)
    label = f"{category}/" if category != 'kryos' else ''
    return f"Added {label}{name} = \"{version}\""


def remove_dependency(
    name: str,
    category: str = 'kryos',
    project_dir: Optional[str] = None,
) -> str:
    """Remove a dependency from kryos.toml."""
    config = load_config(project_dir)
    deps = config.get('dependencies', {})

    if category == 'kryos':
        if name in deps:
            del deps[name]
        else:
            return f"Dependency '{name}' not found"
    else:
        sub = deps.get(category, {})
        if isinstance(sub, dict) and name in sub:
            del sub[name]
            # Remove empty sub-table
            if not sub:
                del deps[category]
        else:
            return f"Dependency '{category}/{name}' not found"

    save_config(config, project_dir)
    label = f"{category}/" if category != 'kryos' else ''
    return f"Removed {label}{name}"


def list_dependencies(project_dir: Optional[str] = None) -> Dict[str, Dict[str, str]]:
    """List all dependencies grouped by category.

    Returns dict like:
        {"kryos": {"pkg": "1.0"}, "python": {"numpy": "1.26"}, ...}
    """
    config = load_config(project_dir)
    deps = config.get('dependencies', {})

    result: Dict[str, Dict[str, str]] = {'kryos': {}}

    for key, val in deps.items():
        if isinstance(val, dict):
            # Sub-category like python, rust, c
            result[key] = {k: str(v) for k, v in val.items()}
        else:
            # Top-level kryos dep
            result['kryos'][key] = str(val)

    return result


def resolve_dependencies(project_dir: Optional[str] = None) -> Dict[str, Dict[str, Optional[str]]]:
    """Resolve version constraints against available packages.

    Returns dict of {category: {name: resolved_version_or_None}}.
    """
    deps = list_dependencies(project_dir)
    resolved: Dict[str, Dict[str, Optional[str]]] = {}

    for category, packages in deps.items():
        resolved[category] = {}
        for pkg_name, constraint in packages.items():
            if category == 'kryos':
                best = _resolve_local_package(pkg_name, constraint)
                resolved[category][pkg_name] = best
            else:
                # Foreign deps: we just record the constraint; actual install
                # is delegated to pip/cargo/etc.
                resolved[category][pkg_name] = constraint
    return resolved


def _resolve_local_package(name: str, constraint: str) -> Optional[str]:
    """Find the best matching version of a Kryos package in the local cache."""
    pkg_dir = PACKAGES_DIR / name
    if not pkg_dir.is_dir():
        return None

    versions: List[SemVer] = []
    for child in pkg_dir.iterdir():
        if child.is_dir():
            try:
                v = SemVer(child.name)
                if version_satisfies(child.name, constraint):
                    versions.append(v)
            except (ValueError, IndexError):
                continue

    if not versions:
        return None

    versions.sort(reverse=True)
    return str(versions[0])


def install_dependencies(project_dir: Optional[str] = None) -> List[str]:
    """Install dependencies into the local cache.

    For Kryos packages: copies from registry if available, otherwise reports missing.
    For foreign deps (python/rust/c): prints instructions (does not auto-install).

    Returns list of status messages.
    """
    resolved = resolve_dependencies(project_dir)
    messages: List[str] = []

    # Ensure cache dir exists
    PACKAGES_DIR.mkdir(parents=True, exist_ok=True)

    for category, packages in resolved.items():
        for pkg_name, version in packages.items():
            if category == 'kryos':
                if version is None:
                    messages.append(f"  [MISSING] {pkg_name} — not found in local registry")
                else:
                    messages.append(f"  [OK]      {pkg_name} = {version}")
            elif category == 'python':
                messages.append(f"  [python]  pip install {pkg_name}=={version}")
            elif category == 'rust':
                messages.append(f"  [rust]    cargo add {pkg_name}@{version}")
            elif category == 'c':
                messages.append(f"  [c]       ensure {pkg_name} {version} is available on system")
            else:
                messages.append(f"  [{category}] {pkg_name} = {version}")

    return messages


# ===========================================================================
# Module resolution (for `use` imports)
# ===========================================================================

def resolve_module(module_path: str, project_dir: Optional[str] = None) -> Optional[Path]:
    """Resolve a dotted module path to a .kry file.

    Search order:
    1. Project-local src/ directory
    2. Installed packages in ~/.kryos/packages/

    Example: ``use kryos_ml.transformers``
      -> checks src/kryos_ml/transformers.kry
      -> checks ~/.kryos/packages/kryos_ml/<version>/transformers.kry
    """
    parts = module_path.split('.')
    if not parts:
        return None

    # 1. Project-local
    base = Path(project_dir) if project_dir else Path.cwd()
    local_path = base / 'src' / '/'.join(parts[:-1]) / (parts[-1] + '.kry') if len(parts) > 1 else base / 'src' / (parts[0] + '.kry')
    if local_path.exists():
        return local_path

    # Also try as directory with mod.kry
    local_dir = base / 'src' / '/'.join(parts) / 'mod.kry'
    if local_dir.exists():
        return local_dir

    # 2. Installed packages
    pkg_name = parts[0]
    pkg_base = PACKAGES_DIR / pkg_name
    if pkg_base.is_dir():
        # Find latest version
        version_dirs = sorted(
            [d for d in pkg_base.iterdir() if d.is_dir()],
            key=lambda d: SemVer(d.name) if _is_version(d.name) else SemVer('0'),
            reverse=True,
        )
        for ver_dir in version_dirs:
            sub_parts = parts[1:] if len(parts) > 1 else ['mod']
            candidate = ver_dir / 'src' / '/'.join(sub_parts[:-1]) / (sub_parts[-1] + '.kry') if len(sub_parts) > 1 else ver_dir / 'src' / (sub_parts[0] + '.kry')
            if candidate.exists():
                return candidate
            # Try mod.kry
            mod_candidate = ver_dir / 'src' / '/'.join(sub_parts) / 'mod.kry'
            if mod_candidate.exists():
                return mod_candidate

    return None


def _is_version(s: str) -> bool:
    """Check if a string looks like a version number."""
    return bool(re.match(r'^\d+(\.\d+)*$', s))


# ===========================================================================
# Package publishing (local)
# ===========================================================================

def publish_local(project_dir: Optional[str] = None) -> str:
    """Publish the current project to the local package registry.

    Copies project files into ~/.kryos/packages/<name>/<version>/
    """
    config = load_config(project_dir)
    pkg = config.get('package', {})
    name = pkg.get('name')
    version = pkg.get('version', '0.1.0')

    if not name:
        return "Error: package.name is required in kryos.toml"

    dest = PACKAGES_DIR / name / version
    dest.mkdir(parents=True, exist_ok=True)

    src_dir = Path(project_dir) if project_dir else Path.cwd()

    # Copy kryos.toml
    shutil.copy2(src_dir / CONFIG_FILE, dest / CONFIG_FILE)

    # Copy src/
    src_src = src_dir / 'src'
    if src_src.is_dir():
        dest_src = dest / 'src'
        if dest_src.exists():
            shutil.rmtree(dest_src)
        shutil.copytree(src_src, dest_src)

    return f"Published {name} v{version} to {dest}"


# ===========================================================================
# Helpers
# ===========================================================================

def _deep_copy_dict(d: dict) -> dict:
    """Deep copy a nested dict (no external dep needed)."""
    out = {}
    for k, v in d.items():
        if isinstance(v, dict):
            out[k] = _deep_copy_dict(v)
        elif isinstance(v, list):
            out[k] = list(v)
        else:
            out[k] = v
    return out
