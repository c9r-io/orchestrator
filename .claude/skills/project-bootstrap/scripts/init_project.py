#!/usr/bin/env python3
import argparse
import os
import re
import sys
from pathlib import Path

TEXT_EXTENSIONS = {
    ".md", ".toml", ".rs", ".ts", ".tsx", ".js", ".json", ".yml", ".yaml",
    ".env", ".sh", ".txt", ".html", ".css", ".Dockerfile"
}

PLACEHOLDERS = [
    "{{project_name}}",
    "{{core_port}}",
    "{{portal_port}}",
    "{{namespace}}",
]


def is_text_file(path: Path) -> bool:
    if path.suffix in TEXT_EXTENSIONS:
        return True
    if path.name.startswith("Dockerfile"):
        return True
    return False


def replace_placeholders(path: Path, values: dict) -> None:
    if not is_text_file(path):
        return
    try:
        content = path.read_text(encoding="utf-8")
    except Exception:
        return

    for key, val in values.items():
        content = content.replace(key, str(val))

    path.write_text(content, encoding="utf-8")


def normalize_project_name(raw: str) -> str:
    name = raw.strip().lower()
    name = name.replace("_", "-")
    name = re.sub(r"[^a-z0-9-]+", "-", name)
    name = re.sub(r"-{2,}", "-", name).strip("-")
    return name


def validate_project_name(name: str) -> None:
    # Keep it compatible with Kubernetes namespace DNS labels and common image naming.
    # https://kubernetes.io/docs/concepts/overview/working-with-objects/names/#dns-label-names
    if not name:
        raise RuntimeError("Project name is required.")
    if len(name) > 63:
        raise RuntimeError("Project name must be <= 63 characters.")
    if not re.fullmatch(r"[a-z0-9]([a-z0-9-]*[a-z0-9])?", name):
        normalized = normalize_project_name(name)
        hint = f" Try: {normalized}" if normalized and normalized != name else ""
        raise RuntimeError(
            "Invalid project name. Use lowercase letters, digits, and '-' only, "
            "start/end with an alphanumeric, and keep it <= 63 chars." + hint
        )


def copy_tree(src: Path, dst: Path, *, require_empty: bool) -> list[Path]:
    created_files: list[Path] = []
    if dst.exists() and any(dst.iterdir()):
        if require_empty:
            raise RuntimeError(f"Target directory not empty: {dst}")

        # Safe merge mode (default): allow non-empty targets as long as we won't
        # collide with template top-level entries (core/, portal/, etc).
        collisions: list[str] = []
        for child in src.iterdir():
            if (dst / child.name).exists():
                collisions.append(child.name)
        if collisions:
            collisions_str = ", ".join(sorted(collisions))
            raise RuntimeError(
                "Target directory has conflicting paths: "
                f"{collisions_str}. Remove/rename those paths or choose a different target directory."
            )
    for root, dirs, files in os.walk(src):
        rel = Path(root).relative_to(src)
        target_root = dst / rel
        target_root.mkdir(parents=True, exist_ok=True)
        for name in files:
            target_file = target_root / name
            if target_file.exists():
                raise RuntimeError(f"Refusing to overwrite existing file: {target_file}")
            target_file.write_bytes((Path(root) / name).read_bytes())
            created_files.append(target_file)
    return created_files


def main() -> None:
    parser = argparse.ArgumentParser(description="Initialize a new project from templates")
    parser.add_argument("--root", required=True, help="Target directory")
    parser.add_argument(
        "--name",
        required=False,
        help="Project name (e.g., acme). Used for naming, namespace, and image identifiers.",
    )
    parser.add_argument("--core-port", type=int, default=8080)
    parser.add_argument("--portal-port", type=int, default=3000)
    parser.add_argument("--namespace", default=None)
    parser.add_argument(
        "--require-empty",
        action="store_true",
        help="Require the target directory to be empty (strict mode).",
    )
    args = parser.parse_args()

    skill_dir = Path(__file__).resolve().parents[1]
    template_dir = skill_dir / "assets" / "template"

    if not template_dir.exists():
        raise RuntimeError(f"Template directory not found: {template_dir}")

    target_dir = Path(args.root).resolve()
    target_dir.mkdir(parents=True, exist_ok=True)

    if not args.name:
        default_name = target_dir.name
        if sys.stdin.isatty():
            if default_name:
                entered = input(f"Project name [{default_name}]: ").strip()
                args.name = entered or default_name
            else:
                args.name = input("Project name: ").strip()
        else:
            # Non-interactive: refuse to hang; infer from directory name if possible.
            if not default_name:
                raise RuntimeError(
                    "Missing --name and cannot infer a default from --root. Provide --name."
                )
            args.name = default_name

    validate_project_name(args.name)
    namespace = args.namespace or args.name

    created_files = copy_tree(template_dir, target_dir, require_empty=args.require_empty)

    values = {
        "{{project_name}}": args.name,
        "{{core_port}}": args.core_port,
        "{{portal_port}}": args.portal_port,
        "{{namespace}}": namespace,
    }

    # Only replace placeholders in files we just created. This prevents clobbering
    # pre-existing repo content when --root points at an existing repository.
    for path in created_files:
        replace_placeholders(path, values)

    print("Project initialized:")
    print(f"  name: {args.name}")
    print(f"  root: {target_dir}")
    print(f"  core port: {args.core_port}")
    print(f"  portal port: {args.portal_port}")
    print(f"  namespace: {namespace}")


if __name__ == "__main__":
    main()
